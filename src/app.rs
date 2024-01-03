use crate::buffer::{Read, ReadBuffer};
use crate::io::bifastq::BidirectionalFastqReader;
use crate::read_stylizing::highlight_matches;

use bio::io::fastq;
use bio::pattern_matching::myers::{BitVec, Myers, MyersBuilder};
use interval::interval_set::ToIntervalSet;
use interval::IntervalSet;
use ratatui::prelude::{Alignment, Color, Line, Modifier, Rect, Span, Style, Stylize};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use rayon::prelude::*;
use std::fs::File;
use std::path::{Path, PathBuf};
use tui_textarea::{CursorMove, TextArea};

#[derive(Debug)]
pub struct App<'a> {
    pub quit: bool,
    pub search_patterns: Vec<SearchPattern>,
    pub records: ReadBuffer<'a>,

    pub mode: UIMode,
    pub line_num: usize, // x2 of records_buf (id + seq)
    pub search_panel: SearchPanel<'a>,
    // file: PathBuf,
    // reader: BidirectionalFastqReader<File>,
    // buf_bounded: (bool, bool), // buffer reached start / end of file
    active_boarder_style: Style,
    pub scroll_status: (usize, u16),
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

impl App<'_> {
    pub fn new(file: String) -> Self {
        let default_search_patterns = vec![
            SearchPattern::new("CTACACGACGCTCTTCCGATCT".to_string(), Color::Blue, 3),
            SearchPattern::new("AGATCGGAAGAGCGTCGTGTAG".to_string(), Color::Green, 3),
            SearchPattern::new("TTTTTTTTTTTT".to_string(), Color::Blue, 0),
            SearchPattern::new("AAAAAAAAAAAA".to_string(), Color::Green, 0),
        ];
        let mut instance = App {
            quit: false,
            search_patterns: default_search_patterns,
            records: ReadBuffer::new(file),
            mode: UIMode::Viewer(SearchPanelState {
                focus: SearchPanelFocus::PatternsList,
                patterns_list_selection: Some(0),
            }),
            line_num: 0,
            search_panel: SearchPanel::new(&default_search_patterns),
            active_boarder_style: Style::new().red().bold(),
            scroll_status: (0, 0),
        };
        instance.highligh_search_panel_focus(SearchPanelFocus::PatternsList);
        instance.update(true);
        instance
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn set_search_patterns(&mut self, search_patterns: Vec<SearchPattern>) {
        self.search_patterns = search_patterns;
        self.update(true);
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
        self.update(true);
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
        self.update(true);
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
                .title("Search string (ALT-2)"),
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

    pub fn scroll(&mut self, n: isize, rect: Rect) {
        // current location:
        let width = rect.width;
        let mut read = self.scroll_status.0 as i16;
        let line = u16::try_from(self.scroll_status.1).unwrap();

        // do we need to scroll up?
        if n < 0 {
            let mut n_up = u16::try_from(-n).unwrap(); // easier to think of a positive number of lines to scroll up

            // are we staying within the same read?
            if n_up <= line {
                self.scroll_status.1 -= n_up;
                return;
            }
            n_up -= line;

            // if not, let's scroll 'up'
            read -= 1;
            loop {
                if read < 0 {
                    // reached the end of the file, there is no scrolling to be done
                    self.scroll_status = (0, 0);
                    return;
                }

                let new_read = self
                    .records
                    .get_index(read as usize)
                    .expect("This should never fail");
                let height = new_read.calculate_height(width);
                if n_up <= height {
                    self.scroll_status = (read as usize, height - n_up);
                    return;
                }
                n_up -= height;
                read -= 1;
            }
        } else {
            let mut n = u16::try_from(n).unwrap(); // type conversion

            // are we staying within the same read?
            let height = self
                .records
                .get_index(read as usize)
                .expect("Reading should never fail from the current read")
                .calculate_height(width);
            if n + line <= height {
                self.scroll_status.1 += n;
                return;
            }

            // if not, let's scroll up a bit
            n -= height - line;
            read += 1;
            loop {
                if read < 0 {
                    // reached the end of the file, there is no scrolling to be done
                    self.scroll_status = (0, 0);
                    return;
                }
                if let Some(new_read) = self.records.get_index(read as usize) {
                    let height = new_read.calculate_height(width);
                    if n <= height {
                        self.scroll_status = (read as usize, n);
                        return;
                    }
                    n -= height;
                    read += 1;
                }
            }
        }

        // always make sure to have 5 reads buffered
    }

    pub fn back_to_top(&mut self) {
        todo!();
        // self.reader.rewind_to_start().unwrap();
        // self.records_buf = self
        //     .reader
        //     .next_n(RECORDS_BUF_SIZE)
        //     .unwrap()
        //     .into_iter()
        //     .map(|x| Read::new(x))
        //     .collect();

        // self.line_num = 0;
        // self.buf_bounded = (true, self.records_buf.len() < RECORDS_BUF_SIZE);
        // self.update();
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

    pub fn message(&mut self, msg: String) {
        self.search_panel.input_button =
            Paragraph::new("ALT-5 to add pattern\n".to_string() + &*msg)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL));
    }

    pub fn update(&mut self, patterns_changed: bool) {
        // parallel by record ver
        self.records.reads.par_iter_mut()
            .filter(|read| read.lines.is_none() || patterns_changed)
            .for_each(|read| {
            let seq = String::from_utf8_lossy(read.read.seq()).to_string();
            let matches: Vec<(IntervalSet<usize>, Color)> = self
                .search_patterns
                .iter()
                .map(|x| (Self::search(&read.read, x), x.color))
                .collect::<Vec<(IntervalSet<usize>, Color)>>();

            read.lines = Some(vec![
                Line::raw(read.read.id().to_string()),
                highlight_matches(&matches, seq, Color::Gray),
            ]);
        });
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
