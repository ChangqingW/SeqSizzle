use crate::io::bifastq::BidirectionalFastqReader;
use crate::read_stylizing::highlight_matches;

use bio::io::fastq;
use bio::pattern_matching::myers::Myers;
use interval::interval_set::ToIntervalSet;
use interval::IntervalSet;
use ratatui::prelude::{Alignment, Color, Line, Modifier, Rect, Span, Style, Stylize};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use std::fs::File;
use std::path::{Path, PathBuf};
use tui_textarea::TextArea;
use rayon::prelude::*;

const RECORDS_BUF_SIZE: usize = 100; // Need to be a multiple of 4

#[derive(Debug)]
pub struct App<'a> {
    pub quit: bool,
    pub search_patterns: Vec<SearchPattern>,
    pub records_buf: Vec<fastq::Record>,
    pub line_buf: Vec<Line<'a>>,
    pub mode: UIMode,
    pub line_num: usize, // x2 of records_buf (id + seq)
    pub search_panel: SearchPanel<'a>,
    file: PathBuf,
    reader: BidirectionalFastqReader<File>,
    active_boarder_style: Style,
    buf_bounded: (bool, bool), // buffer reached start / end of file
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
        let mut reader = BidirectionalFastqReader::from_path(&Path::new(&file));
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
            records_buf: reader
                .next_n(RECORDS_BUF_SIZE)
                .expect("Failed to parse record"),
            line_buf: Vec::new(),
            mode: UIMode::Viewer(SearchPanelState {
                focus: SearchPanelFocus::PatternsList,
                patterns_list_selection: Some(0),
            }),
            line_num: 0,
            search_panel: SearchPanel::new(&default_search_patterns),
            file: Path::new(&file).to_path_buf(),
            reader,
            active_boarder_style: Style::new().red().bold(),
            buf_bounded: (true, false),
        };
        if instance.records_buf.len() < RECORDS_BUF_SIZE {
            instance.buf_bounded.1 = true;
        }
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
    }

    pub fn toggle_ui_mode(&mut self) {
        match &self.mode {
            UIMode::Viewer(focus) => self.mode = UIMode::SearchPanel(*focus),
            UIMode::SearchPanel(focus) => self.mode = UIMode::Viewer(*focus),
        };
    }

    fn scrollable_lines(&self, tui_size: Rect) -> usize {
        let mut lines = 0;
        for i in (self.line_num..self.line_buf.len()).rev() {
            lines += (self.line_buf[i].width() as u16 + tui_size.width - 1) / tui_size.width;
            if lines + 1 >= tui_size.height {
                return i - self.line_num + 1;
            }
        }
        0
    }

    fn buffer_forward(&mut self) {
        let mut new_records = self.reader.next_n(RECORDS_BUF_SIZE / 4).unwrap();
        let len = new_records.len();
        if len < RECORDS_BUF_SIZE / 4 {
            self.buf_bounded.1 = true;
        }
        self.records_buf.append(&mut new_records);
        self.records_buf =
            self.records_buf[self.records_buf.len().saturating_sub(RECORDS_BUF_SIZE)..].to_vec();
        self.line_num = self.line_num.saturating_sub(len * 2);
        if self.buf_bounded.0 && len > 0 {
            self.buf_bounded.0 = false;
        }
        self.update();
    }

    fn buffer_backward(&mut self) {
        let mut new_records = self
            .reader
            .prev_n(self.records_buf.len() + (RECORDS_BUF_SIZE / 4))
            .unwrap();
        let len = new_records.len();
        self.reader.rewind_n(len - RECORDS_BUF_SIZE).unwrap();
        if len < self.records_buf.len() + (RECORDS_BUF_SIZE / 4) {
            self.buf_bounded.0 = true;
        }
        new_records.append(&mut self.records_buf);
        self.records_buf = new_records[..RECORDS_BUF_SIZE.min(new_records.len())].to_vec();
        self.line_num += (len - RECORDS_BUF_SIZE) * 2;
        self.update();
    }

    pub fn scroll(&mut self, num: isize, tui_rect: Rect) {
        if num <= isize::MIN + 1 {
            self.back_to_top();
            return;
        }
        let scrollable_lines = self.scrollable_lines(tui_rect);
        if num < 0 && (num.abs() as usize > self.line_num) {
            match self.buf_bounded.0 {
                true => self.line_num = 0,
                false => {
                    self.buffer_backward();
                    self.scroll(num, tui_rect);
                }
            }
        } else if num > scrollable_lines as isize {
            match self.buf_bounded.1 {
                true => self.line_num += scrollable_lines,
                false => {
                    self.buffer_forward();
                    self.scroll(num, tui_rect);
                }
            }
        } else {
            self.line_num = (self.line_num as isize + num) as usize;
        }

        if scrollable_lines <= 1 && !self.buf_bounded.1 {
            self.buffer_forward();
        } else if self.line_num <= 1 && !self.buf_bounded.0 {
            self.buffer_backward();
        }
    }

    pub fn back_to_top(&mut self) {
        self.reader.rewind_to_start().unwrap();
        self.records_buf = self.reader.next_n(RECORDS_BUF_SIZE).unwrap();
        self.line_num = 0;
        self.buf_bounded = (true, self.records_buf.len() < RECORDS_BUF_SIZE);
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

    pub fn message(&mut self, msg: String) {
        self.search_panel.input_button =
            Paragraph::new("ALT-5 to add pattern\n".to_string() + &*msg)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL));
    }

    pub fn update_parallel_inner_ver(&mut self) {
        let mut result: Vec<Line> = Vec::new();
        for record in &self.records_buf {
            let seq = String::from_utf8_lossy(record.seq()).to_string();

            let mut matches: Vec<(IntervalSet<usize>, Color)> = Vec::new();
            self.search_patterns
                .par_iter()
                .map(|x| {
                    let mut myers = Myers::<u64>::new(x.search_string.clone().into_bytes());
                    (
                        myers
                            .find_all(record.seq(), x.edit_distance)
                            .map(|(a, b, _)| (a, b - 1))
                            .collect::<Vec<(usize, usize)>>()
                            .to_interval_set(),
                        x.color,
                    )
                })
                .collect_into_vec(&mut matches);
            result.push(record.id().to_string().into());
            result.push(highlight_matches(&matches, seq, Color::Gray));
        }
        self.line_buf = result;
    }

    pub fn update(&mut self) { // parallel by record ver
        self.line_buf = self
            .records_buf
            .par_iter()
            .map(|record| {
                let seq = String::from_utf8_lossy(record.seq()).to_string();
                let matches: Vec<(IntervalSet<usize>, Color)> = self
                    .search_patterns
                    .iter()
                    .map(|x| {
                        let mut myers = Myers::<u64>::new(x.search_string.clone().into_bytes());
                        (
                            myers
                                .find_all(record.seq(), x.edit_distance)
                                .map(|(a, b, _)| (a, b - 1))
                                .collect::<Vec<(usize, usize)>>()
                                .to_interval_set(),
                            x.color,
                        )
                    })
                    .collect::<Vec<(IntervalSet<usize>, Color)>>();
                (
                    record.id().to_string().into(),
                    highlight_matches(&matches, seq, Color::Gray),
                )
            })
            .flat_map(|(id, seq)| vec![id, seq])
            .collect();
    }
}
