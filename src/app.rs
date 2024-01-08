use crate::io::fastq::FastqReader;
use crate::read_stylizing::highlight_matches;

use bio::io::fastq;
use bio::pattern_matching::myers::{BitVec, Myers, MyersBuilder};
use interval::interval_set::ToIntervalSet;
use interval::IntervalSet;
use ratatui::prelude::{Alignment, Color, Line, Modifier, Rect, Span, Style, Stylize};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use rayon::prelude::*;
use std::fs::File;
use std::path::{Path, PathBuf};
use tui_textarea::{CursorMove, TextArea};
use std::collections::VecDeque;

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
    active_boarder_style: Style,
    message: TransientMessage,
}

#[derive(Debug, Clone)]
pub struct SearchPattern {
    pub search_string: String,
    pub color: Color,
    pub edit_distance: u8,
}
impl SearchPattern {
    pub fn new(search_string: String, color: Color, edit_distance: u8) -> Self {
        Self {
            search_string,
            color,
            edit_distance,
        }
    }
}

#[derive(Debug)]
pub struct SearchPanel<'a> {
    pub patterns_list: List<'a>,
    pub input_pattern: TextArea<'a>,
    pub input_color: TextArea<'a>,
    pub input_distance: TextArea<'a>,
    pub input_button: Paragraph<'a>, // dyn Widget
}

fn search_patterns_to_list<'a>(search_patterns: &[SearchPattern]) -> List<'a> {
    List::new(
        search_patterns
            .iter()
            .map(|x| {
                ListItem::new(Line::from(vec![
                    Span::from(x.search_string.clone()),
                    Span::from(", color: "),
                    Span::styled(x.color.to_string(), Style::new().fg(x.color)),
                    Span::from(format!(", edit-distance: {}", x.edit_distance)),
                ]))
            })
            .collect::<Vec<ListItem>>(),
    )
    .block(
        Block::default()
            .title("Search patterns (ALT-1 to switch)")
            .borders(Borders::ALL),
    )
    .highlight_style(Style::default().add_modifier(Modifier::BOLD))
    .highlight_symbol("> ")
}
impl SearchPanel<'_> {
    pub fn new(search_patterns: &[SearchPattern]) -> Self {
        let mut ret = Self {
            patterns_list: search_patterns_to_list(search_patterns),
            input_pattern: TextArea::default(),
            input_color: TextArea::default(),
            input_distance: TextArea::default(),
            input_button: Paragraph::new(
                "ALT-5 to add pattern\n(Todo: make clickable button and input fields)",
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL)),
        };
        ret.input_pattern.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Search string (ALT-2)"),
        );
        ret.input_color.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Color (ALT-3)"),
        );
        ret.input_distance.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Edit distance (ALT-4)"),
        );
        ret
    }
}

#[derive(Debug, PartialEq)]
pub enum UIMode {
    Viewer(SearchPanelState), // memorize search panel state
    SearchPanel(SearchPanelState),
}
impl UIMode {
    pub fn get_search_panel_state(&self) -> &SearchPanelState {
        match self {
            UIMode::Viewer(state) => state,
            UIMode::SearchPanel(state) => state,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Eq, Hash, Copy)]
pub enum SearchPanelFocus {
    PatternsList,
    InputPattern,
    InputColor,
    InputDistance,
    InputButton,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash, Copy)]
pub struct SearchPanelState {
    pub focus: SearchPanelFocus,
    pub patterns_list_selection: Option<usize>,
}

#[derive(Debug)]
pub struct TransientMessage {
    message: String,
    timer: u8, // ticks to live
}
impl TransientMessage {
    pub fn new(message: String) -> Self {
        Self {
            message,
            timer: 1,
        }
    }
    pub fn default() -> Self {
        Self {
            message: String::new(),
            timer: 0,
        }
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
    pub fn new(file: String) -> Self {
        let reader = FastqReader::from_path(&Path::new(&file));
        let default_search_patterns = vec![
            SearchPattern::new("CTACACGACGCTCTTCCGATCT".to_string(), Color::Blue, 3),
            SearchPattern::new("AGATCGGAAGAGCGTCGTGTAG".to_string(), Color::Green, 3),
            SearchPattern::new("TTTTTTTTTTTT".to_string(), Color::Blue, 0),
            SearchPattern::new("AAAAAAAAAAAA".to_string(), Color::Green, 0),
            SearchPattern::new("TCTTCTTTC".to_string(), Color::Red, 0),
        ];
        let mut instance = App {
            quit: false,
            search_patterns: default_search_patterns.clone(),
            message: TransientMessage::default(),
            mode: UIMode::Viewer(SearchPanelState {
                focus: SearchPanelFocus::PatternsList,
                patterns_list_selection: Some(0),
            }),
            search_panel: SearchPanel::new(&default_search_patterns),
            file: Path::new(&file).to_path_buf(),
            reader,
            active_boarder_style: Style::new().red().bold(),
            rendered_lines: VecDeque::with_capacity(2*(RENDER_BUF_SIZE + 1)),
            scroll_status: (0, 0)
        };
        instance.highligh_search_panel_focus(SearchPanelFocus::PatternsList);
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
        self.search_panel = SearchPanel::new(&self.search_patterns);
        match &self.mode {
            UIMode::SearchPanel(SearchPanelState { focus, .. }) => {
                self.highligh_search_panel_focus(*focus)
            }
            any => panic!("Unexpected UI mode {:?}", any),
        };
        self.update();
    }

    pub fn delete_search_pattern(&mut self, index: usize) -> SearchPattern {
        let pattern = self.search_patterns.remove(index);
        self.search_panel = SearchPanel::new(&self.search_patterns);
        match &self.mode {
            UIMode::SearchPanel(SearchPanelState { focus, .. }) => {
                self.highligh_search_panel_focus(*focus)
            }
            any => panic!("Unexpected UI mode {:?}", any),
        };
        self.update();
        if self.search_patterns.len() == 0 {
            self.mode = UIMode::SearchPanel(SearchPanelState {
                focus: SearchPanelFocus::PatternsList,
                patterns_list_selection: None,
            });
        } else if index >= self.search_patterns.len() {
            self.mode = UIMode::SearchPanel(SearchPanelState {
                focus: SearchPanelFocus::PatternsList,
                patterns_list_selection: Some(self.search_patterns.len() - 1),
            });
        }
        return pattern;
    }

    pub fn edit_search_pattern(&mut self, index: usize) {
        let pattern: SearchPattern = self.delete_search_pattern(index);
        self.search_panel.input_pattern = TextArea::new(vec![pattern.search_string]);
        self.search_panel.input_color = TextArea::new(vec![pattern.color.to_string()]);
        self.search_panel.input_distance = TextArea::new(vec![pattern.edit_distance.to_string()]);
        self.search_panel.input_pattern.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Search string (ALT-2)"), // TODO: move search panel title & boarder to
                                                 // ui.rs?
        );
        self.search_panel.input_color.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Color (ALT-3)"),
        );
        self.search_panel.input_distance.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Edit distance (ALT-4)"),
        );

        self.search_panel
            .input_pattern
            .move_cursor(CursorMove::Bottom);
        self.search_panel
            .input_color
            .move_cursor(CursorMove::Bottom);
        self.search_panel
            .input_distance
            .move_cursor(CursorMove::Bottom);
        self.search_panel.input_pattern.move_cursor(CursorMove::End);
        self.search_panel.input_color.move_cursor(CursorMove::End);
        self.search_panel
            .input_distance
            .move_cursor(CursorMove::End);
    }

    pub fn toggle_ui_mode(&mut self) {
        match &self.mode {
            UIMode::Viewer(focus) => self.mode = UIMode::SearchPanel(*focus),
            UIMode::SearchPanel(focus) => self.mode = UIMode::Viewer(*focus),
        };
    }

    pub fn scroll(&mut self, num: isize, tui_rect: Rect) {
        /// scroll the rendered lines by num
        /// rendered_lines append / pop lines if scrolling beyond a read

        // line height in tui
        fn line_height(line: &Line, tui_rect: Rect) -> usize {
            return line.width().div_ceil(tui_rect.width as usize - 2); // 2 boarders 1 char wide
        }
        fn lines_height_vec(lines: &[Line], tui_rect: Rect) -> usize {
            return lines.iter().map(|x| line_height(x, tui_rect)).sum();
        }
        fn lines_height_vecdeque(lines: &VecDeque<Line>, indexes: &[usize], tui_rect: Rect) -> usize {
            return indexes.iter().map(|x| line_height(&lines[*x], tui_rect)).sum();
        }

        if num == 0 {
            return;
        } else if num <= isize::MIN + 1 {
            self.back_to_top();
            return;
        } else if num < 0 {
            if self.scroll_status.1 > 0 { // scroll within the first line 
                let remaining = self.scroll_status.1 as isize + num;
                self.scroll_status.1 = remaining.max(0) as usize;
                return self.scroll(remaining.min(0), tui_rect);
            } else {
                let mut remaining = num;
                while remaining < 0 && self.scroll_status.0 > 0 {
                    let lines = Self::record_to_lines(
                        &self.reader.get_index(self.scroll_status.0 - 1)
                        .unwrap().expect("Failed to fetch previous record while scroll_status.0 > 1"),
                        &self.search_patterns);
                    remaining += lines_height_vec(&lines[0..2], tui_rect) as isize;
                    lines.into_iter().rev().for_each(|x| self.rendered_lines.push_front(x));
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
            let mut current_line_height = lines_height_vecdeque(&self.rendered_lines, &vec![0, 1], tui_rect);
            self.scroll_status.1 = 0;

            while remaining >= current_line_height as isize {
                let rec = self.reader.get_index(self.scroll_status.0 + RENDER_BUF_SIZE).unwrap();
                if rec.is_none() { // EOF reached, scroll the rendered lines within their total height
                    let max_scroll = 3 + self.rendered_lines // 2 x boarders 1 char high, plus 1 empty line to indicate EOF
                        .iter()
                        .map(|x| line_height(&x, tui_rect))
                        .sum::<usize>()
                        .saturating_sub(tui_rect.height as usize); 
                    self.scroll_status.1 = (self.scroll_status.1 + remaining as usize).min(max_scroll);
                    if self.scroll_status.1 == max_scroll {
                        self.set_message("Hit bottom".to_string());
                    }
                    return;
                }
                // otherwise append new line and pop current line
                self.rendered_lines.pop_front().expect("Failed to pop front line id");
                self.rendered_lines.pop_front().expect("Failed to pop front line seq");
                self.scroll_status.0 += 1;
                Self::record_to_lines(&rec.unwrap(), &self.search_patterns)
                    .into_iter()
                    .for_each(|x| self.rendered_lines.push_back(x));
                remaining -= current_line_height as isize;
                current_line_height = lines_height_vecdeque(&self.rendered_lines, &vec![0, 1], tui_rect);
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

    fn highligh_search_panel_focus(&mut self, focus: SearchPanelFocus) {
        match self.mode.get_search_panel_state().focus {
            SearchPanelFocus::PatternsList => {
                self.search_panel.patterns_list = self.search_panel.patterns_list.clone().block(
                    Block::default()
                        .title("Search patterns (ALT-1 to switch)")
                        .borders(Borders::ALL),
                )
            }
            SearchPanelFocus::InputPattern => self.search_panel.input_pattern.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Search string (ALT-2)"),
            ),
            SearchPanelFocus::InputColor => self.search_panel.input_color.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Color (ALT-3)"),
            ),
            SearchPanelFocus::InputDistance => self.search_panel.input_distance.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Edit distance (ALT-4)"),
            ),
            SearchPanelFocus::InputButton => (),
        };
        match focus {
            SearchPanelFocus::PatternsList => {
                self.search_panel.patterns_list = self.search_panel.patterns_list.clone().block(
                    Block::default()
                        .title("Search patterns (ALT-[number] or left / right arrow keys to switch between boxes, up / down to select patterns, enter to edit pattern, delete to delete pattern)")
                        .borders(Borders::ALL)
                        .border_style(self.active_boarder_style),
                )
            }
            SearchPanelFocus::InputPattern => self.search_panel.input_pattern.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.active_boarder_style)
                    .title("Search string"),
            ),
            SearchPanelFocus::InputColor => self.search_panel.input_color.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.active_boarder_style)
                    .title("Color"),
            ),
            SearchPanelFocus::InputDistance => self.search_panel.input_distance.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.active_boarder_style)
                    .title("Edit distance"),
            ),
            SearchPanelFocus::InputButton => (),
        };
    }

    pub fn focus_search_panel(&mut self, focus: SearchPanelFocus) {
        self.highligh_search_panel_focus(focus);
        self.mode = UIMode::SearchPanel(SearchPanelState {
            focus,
            patterns_list_selection: self.mode.get_search_panel_state().patterns_list_selection,
        });
    }

    pub fn cycle_patterns_list(&mut self, reverse: bool) {
        if self.search_patterns.len() == 0 {
            self.mode = UIMode::SearchPanel(SearchPanelState {
                focus: self.mode.get_search_panel_state().focus,
                patterns_list_selection: None,
            });
            return;
        }
        let new_selection = match self.mode.get_search_panel_state().patterns_list_selection {
            Some(selection) => {
                if reverse {
                    Some(
                        selection
                            .checked_sub(1)
                            .unwrap_or(self.search_patterns.len() - 1),
                    )
                } else {
                    if selection == self.search_patterns.len() - 1 {
                        Some(0)
                    } else {
                        Some(selection + 1)
                    }
                }
            }
            None => {
                if reverse {
                    Some(self.search_patterns.len() - 1)
                } else {
                    Some(0)
                }
            }
        };
        self.mode = UIMode::SearchPanel(SearchPanelState {
            focus: self.mode.get_search_panel_state().focus,
            patterns_list_selection: new_selection,
        });
    }

    pub fn set_message(&mut self, msg: String) {
        self.message = TransientMessage::new(msg);
        // eprint!("\x07"); // sending BEL to terminal result in delayed rendering?
    }

    pub fn get_message(&mut self) -> Option<String> {
        self.message.get()
    }

    pub fn resized_update(&mut self, tui_rect: Rect) {
        // TODO
        self.scroll_status.1 = 0;
    }

    /// full update
    /// get lines from reader and render
    pub fn update(&mut self) {
        let records = (self.scroll_status.0 .. self.scroll_status.0 + RENDER_BUF_SIZE)
            .filter_map(|i| {
                self.reader.get_index(i).expect("Failed to get index")
            })
            .collect::<Vec<fastq::Record>>();
        if records.len() < RENDER_BUF_SIZE {
            self.set_message(format!("EOF reached during app.update, {} records rendered", records.len()));
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
            .map(|x| (Self::search(record, x), x.color))
            .collect::<Vec<(IntervalSet<usize>, Color)>>();
        vec![
            record.id().to_string().into(),
            highlight_matches(&matches, seq, Color::Gray),
        ]
    }

    fn search(record: &fastq::Record, pattern: &SearchPattern) -> IntervalSet<usize> {
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
    ) -> IntervalSet<usize>
    where
        <T as BitVec>::DistType: From<u8>,
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
        myers
            .find_all(record.seq(), pattern.edit_distance.into())
            .map(|(a, b, _)| (a, b - 1))
            .collect::<Vec<(usize, usize)>>()
            .to_interval_set()
    }
}
