use crate::io::fastq::FastqReader;
use crate::read_stylizing::highlight_matches;
use crate::search_panel::SearchPanel;

use bio::io::fastq;
use bio::pattern_matching::myers::{BitVec, Myers, MyersBuilder};
use interval::interval_set::ToIntervalSet;
use interval::IntervalSet;
use ratatui::prelude::{Color, Line, Size};

use rayon::prelude::*;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[cfg(debug_assertions)]
const RENDER_BUF_SIZE: usize = 24;
#[cfg(not(debug_assertions))]
const RENDER_BUF_SIZE: usize = 100;

#[derive(Debug)]
pub struct App<'a> {
    pub mode: UIMode,
    pub quit: bool,
    pub search_panel: SearchPanel<'a>,
    pub search_patterns: Vec<SearchPattern>,
    pub file: PathBuf,
    pub rendered_lines: VecDeque<Line<'a>>,
    // offset of the rendered lines to the file
    // scroll within the viewed lines -- reset to 0 on resize
    pub scroll_status: (usize, usize),
    reader: FastqReader<File>,
    message: TransientMessage,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SearchPattern {
    pub search_string: String,
    pub color: Color,
    pub edit_distance: u8,
    pub comment: String,
}
impl SearchPattern {
    pub fn new(search_string: String, color: Color, edit_distance: u8, comment: &str) -> Self {
        Self {
            search_string,
            color,
            edit_distance,
            comment: comment.to_string(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum UIMode {
    Viewer,
    SearchPanel(bool), // bool: save file popup
}

#[derive(Default, Debug)]
pub struct TransientMessage {
    message: String,
    timer: u8, // ticks to live
}

impl TransientMessage {
    pub fn new(message: String) -> Self {
        Self { message, timer: 1 }
    }
    pub fn get(&mut self) -> Option<String> {
        if self.timer > 0 {
            self.timer -= 1;
            Some(self.message.clone())
        } else {
            None
        }
    }
    pub fn dismiss(&mut self) {
        self.timer = 0;
    }
}

impl App<'_> {
    pub fn new(file: &Path, search_patterns: Vec<SearchPattern>) -> Self {
        let reader = FastqReader::from_path(file);
        let mut instance = App {
            quit: false,
            search_patterns: search_patterns.clone(),
            message: TransientMessage::default(),
            mode: UIMode::Viewer,
            search_panel: SearchPanel::new(&search_patterns),
            file: Path::new(&file).to_path_buf(),
            reader,
            rendered_lines: VecDeque::with_capacity(2 * (RENDER_BUF_SIZE + 1)),
            scroll_status: (0, 0),
        };
        instance.update();
        instance
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn set_search_patterns(&mut self, search_patterns: Vec<SearchPattern>) {
        self.search_patterns = search_patterns;
        self.update();
    }

    pub fn append_search_pattern(&mut self, pattern: SearchPattern) {
        self.search_patterns.push(pattern);
        self.search_panel.clear_inputs();
        self.search_panel.update(&self.search_patterns);
        self.update();
    }

    pub fn delete_search_pattern(&mut self, index: usize) -> SearchPattern {
        let pattern = self.search_patterns.remove(index);
        self.search_panel.update(&self.search_patterns);
        self.update();
        pattern
    }

    pub fn edit_search_pattern(&mut self, index: usize) {
        let pattern: SearchPattern = self.delete_search_pattern(index);
        self.search_panel.edit_pattern(pattern);
    }

    pub fn toggle_ui_mode(&mut self) {
        match &self.mode {
            UIMode::Viewer => self.mode = UIMode::SearchPanel(false),
            UIMode::SearchPanel(_) => self.mode = UIMode::Viewer,
        };
    }

    pub fn save_patterns(&self) -> Option<String> {
        let path = self.search_panel.file_save_popup_lines();
        if path.len() != 1 {
            return Some(String::from("Malformed file path"));
        }
        let path = Path::new(&path[0]);
        let file = OpenOptions::new().write(true).create_new(true).open(path);
        if let Err(e) = file {
            match e.kind() {
                std::io::ErrorKind::NotFound => Some(String::from("File path not found")),
                std::io::ErrorKind::PermissionDenied => Some(String::from("Permission denied")),
                std::io::ErrorKind::AlreadyExists => Some(String::from("File already exists")),
                _ => panic!("Unexpected error while saving search patterns: {:?}", e),
            }
        } else {
            let mut writer = csv::Writer::from_writer(file.unwrap());
            writer
                .write_record(["pattern", "color", "editdistance", "comment"])
                .expect("Error writing pattern CSV file headers");
            self.search_patterns.iter().for_each(|pattern| {
                writer
                    .write_record(&[
                        pattern.search_string.clone(),
                        pattern.color.to_string(),
                        pattern.edit_distance.to_string(),
                        pattern.comment.to_string(),
                    ])
                    .expect("Error writing pattern CSV file record");
            });
            if writer.flush().is_err() {
                return Some(String::from("Error flushing pattern CSV file"));
            }
            None
        }
    }

    pub fn scroll(&mut self, num: isize, tui_size: Size) {
        /// scroll the rendered lines by num
        /// rendered_lines append / pop lines if scrolling beyond a read

        // line height in tui
        fn line_height(line: &Line, tui_size: Size) -> usize {
            line.width().div_ceil(tui_size.width as usize - 2) // 2 boarders 1 char wide
        }
        fn lines_height_vec(lines: &[Line], tui_size: Size) -> usize {
            return lines.iter().map(|x| line_height(x, tui_size)).sum();
        }
        fn lines_height_vecdeque(
            lines: &VecDeque<Line>,
            indexes: &[usize],
            tui_size: Size,
        ) -> usize {
            return indexes
                .iter()
                .map(|x| line_height(&lines[*x], tui_size))
                .sum();
        }

        if num == 0 {
            return;
        } else if num <= isize::MIN + 1 {
            self.back_to_top();
            return;
        } else if num < 0 {
            if self.scroll_status.1 > 0 {
                // scroll within the first line
                let remaining = self.scroll_status.1 as isize + num;
                self.scroll_status.1 = remaining.max(0) as usize;
                return self.scroll(remaining.min(0), tui_size);
            } else {
                let mut remaining = num;
                while remaining < 0 && self.scroll_status.0 > 0 {
                    let lines = Self::record_to_lines(
                        &self
                            .reader
                            .get_index(self.scroll_status.0 - 1)
                            .unwrap()
                            .expect("Failed to fetch previous record while scroll_status.0 > 1"),
                        &self.search_patterns,
                    );
                    remaining += lines_height_vec(&lines[0..2], tui_size) as isize;
                    lines
                        .into_iter()
                        .rev()
                        .for_each(|x| self.rendered_lines.push_front(x));
                    self.scroll_status.0 -= 1;
                    if self.rendered_lines.len() > RENDER_BUF_SIZE * 2 {
                        self.rendered_lines.pop_back();
                        self.rendered_lines.pop_back();
                    }
                }
                self.scroll_status.1 = remaining.max(0) as usize;
                if remaining < 0 {
                    self.set_message("Hit top".to_string());
                }
                return;
            }
        } else if num > 0 {
            let mut remaining: isize = num + self.scroll_status.1 as isize; // remaining lines to scroll
            let mut current_line_height =
                lines_height_vecdeque(&self.rendered_lines, &[0, 1], tui_size);
            self.scroll_status.1 = 0;

            while remaining >= current_line_height as isize {
                let rec = self
                    .reader
                    .get_index(self.scroll_status.0 + RENDER_BUF_SIZE)
                    .unwrap();
                if rec.is_none() {
                    // EOF reached, scroll the rendered lines within their total height
                    let max_scroll = 3 + self
                        .rendered_lines // 2 x boarders 1 char high, plus 1 empty line to indicate EOF
                        .iter()
                        .map(|x| line_height(x, tui_size))
                        .sum::<usize>()
                        .saturating_sub(tui_size.height as usize);
                    self.scroll_status.1 =
                        (self.scroll_status.1 + remaining as usize).min(max_scroll);
                    if self.scroll_status.1 == max_scroll {
                        self.set_message("Hit bottom".to_string());
                    }
                    return;
                }
                // otherwise append new line and pop current line
                self.rendered_lines
                    .pop_front()
                    .expect("Failed to pop front line id");
                self.rendered_lines
                    .pop_front()
                    .expect("Failed to pop front line seq");
                self.scroll_status.0 += 1;
                Self::record_to_lines(&rec.unwrap(), &self.search_patterns)
                    .into_iter()
                    .for_each(|x| self.rendered_lines.push_back(x));
                remaining -= current_line_height as isize;
                current_line_height =
                    lines_height_vecdeque(&self.rendered_lines, &[0, 1], tui_size);
            }
            self.scroll_status.1 = remaining as usize;
            return;
        }
        panic!("Unreachable line in scroll");
    }

    pub fn back_to_top(&mut self) {
        self.reader.rewind().unwrap();
        self.scroll_status = (0, 0);
        self.update();
    }

    pub fn cycle_patterns_list(&mut self, reverse: bool) {
        self.search_panel.cycle_patterns_list(reverse);
    }

    pub fn set_message(&mut self, msg: String) {
        self.message = TransientMessage::new(msg);
        // eprint!("\x07"); // sending BEL to terminal result in delayed rendering?
    }

    pub fn get_message(&mut self) -> Option<String> {
        self.message.get()
    }

    pub fn resized_update(&mut self, tui_size: Size) {
        // TODO
        self.scroll_status.1 = 0;
    }

    /// full update
    /// get lines from reader and render
    pub fn update(&mut self) {
        let records = (self.scroll_status.0..self.scroll_status.0 + RENDER_BUF_SIZE)
            .filter_map(|i| self.reader.get_index(i).expect("Failed to get index"))
            .collect::<Vec<fastq::Record>>();
        if records.len() < RENDER_BUF_SIZE {
            self.set_message(format!(
                "EOF reached during app.update, {} records rendered",
                records.len()
            ));
        }
        self.rendered_lines = Self::records_to_lines(&records, &self.search_patterns);
    }

    fn records_to_lines<'a>(
        records: &[fastq::Record],
        search_patterns: &[SearchPattern],
    ) -> VecDeque<Line<'a>> {
        // parallel by record
        records
            .par_iter()
            .map(|record| Self::record_to_lines(record, search_patterns))
            .flatten()
            .collect()
    }

    fn record_to_lines<'a>(
        record: &fastq::Record,
        search_patterns: &[SearchPattern],
    ) -> Vec<Line<'a>> {
        let seq = String::from_utf8_lossy(record.seq()).to_string();
        let matches: Vec<(IntervalSet<usize>, Color)> = search_patterns
            .iter()
            .map(|x| (Self::search(record, x).to_interval_set(), x.color))
            .collect::<Vec<(IntervalSet<usize>, Color)>>();
        vec![
            record.id().to_string().into(),
            highlight_matches(&matches, seq, Color::Gray),
        ]
    }

    pub fn search(record: &fastq::Record, pattern: &SearchPattern) -> Vec<(usize, usize)> {
        if pattern.search_string.len() > 64 {
            panic!("Search pattern need to be less than 64 symbols long");
        }
        if pattern.search_string.len() < 8 {
            Self::search_generic::<u8>(record, pattern)
        } else if pattern.search_string.len() < 16 {
            Self::search_generic::<u16>(record, pattern)
        } else if pattern.search_string.len() < 32 {
            Self::search_generic::<u32>(record, pattern)
        } else {
            Self::search_generic::<u64>(record, pattern)
        }
    }

    fn search_generic<T: BitVec>(
        record: &fastq::Record,
        pattern: &SearchPattern,
    ) -> Vec<(usize, usize)>
    where
        <T as BitVec>::DistType: From<u8> + Into<usize>,
    {
        let mut builder = MyersBuilder::new();
        for (base, equivalents) in vec![
            (b'M', &b"AC"[..]),
            (b'R', &b"AG"[..]),
            (b'W', &b"AT"[..]),
            (b'S', &b"CG"[..]),
            (b'Y', &b"CT"[..]),
            (b'K', &b"GT"[..]),
            (b'V', &b"ACGMRS"[..]),
            (b'H', &b"ACTMWY"[..]),
            (b'D', &b"AGTRWK"[..]),
            (b'B', &b"CGTSYK"[..]),
            (b'N', &b"ACGTMRWSYKVHDB"[..]),
        ] {
            builder.ambig(base, equivalents);
        }

        let mut myers: Myers<T> = builder.build(pattern.search_string.clone().into_bytes());
        let mut matches = myers
            .find_all(record.seq(), pattern.edit_distance.into())
            .map(|(start, end, dist)| (start, end - 1, dist.into()))
            .collect::<Vec<(usize, usize, usize)>>();
        matches.sort_by_key(|(_, _, dist)| *dist);

        // remove greedy fuzzy matches that extends previous matches with mismatches only
        let mut filtered_matches: Vec<(usize, usize, usize)> = Vec::new();
        for m in matches {
            if !filtered_matches.iter().any(|(_, end, dist)| {
                // m.1 - end == m.2 - dist
                m.1 + dist == m.2 + end && m.2 != 0
            }) {
                filtered_matches.push(m);
            }
        }

        filtered_matches
            .into_iter()
            .map(|(start, end, _)| (start, end))
            .collect::<Vec<(usize, usize)>>()
    }
}
