use crate::read_stylizing::highlight_matches;

use bio::io::fastq;
use bio::io::fastq::FastqRead;
use bio::io::fastq::Reader;
use bio::pattern_matching::myers::Myers;
use ratatui::prelude::{Line, Color, Span, Style, Modifier, Stylize, Alignment};
use ratatui::widgets::{List, ListItem, Block, Borders, Paragraph, Wrap};
use interval::interval_set::ToIntervalSet;
use interval::IntervalSet;
use tui_textarea::TextArea;

#[derive(Debug)]
pub struct App<'a> {
    pub quit: bool,
    pub search_patterns: Vec<SearchPattern>,
    pub records_buf: Vec<fastq::Record>,
    pub line_buf: Vec<Line<'a>>,
    pub mode: UIMode,
    pub line_num: usize,
    pub search_panel: SearchPanel<'a>,
    file: String,
    reader: Reader<std::io::BufReader<std::fs::File>>, // buf_size
    active_boarder_style: Style,
}

#[derive(Debug, Clone)]
pub struct SearchPattern {
    pub search_string: String,
    pub color: Color,
    pub edit_distance: u8
}
impl SearchPattern {
    pub fn new(search_string: String, color: Color, edit_distance: u8) -> Self {
        Self {
            search_string,
            color,
            edit_distance
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
    List::new(search_patterns.iter()
        .map(|x| {
            ListItem::new(Line::from(vec![
                Span::from(x.search_string.clone()),
                Span::from(", color: "),
                Span::styled(x.color.to_string(), Style::new().fg(x.color)),
                Span::from(format!(", edit-distance: {}", x.edit_distance))
            ]))
        })
        .collect::<Vec<ListItem>>())
        .block(Block::default().title("Search patterns (ALT-1 to switch)").borders(Borders::ALL))
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
            input_button: Paragraph::new("ALT-5 to add pattern\n(Todo: make clickable button and input fields)").alignment(Alignment::Center).block(Block::default().borders(Borders::ALL))
        };
        ret.input_pattern.
            set_block(Block::default().borders(Borders::ALL).title("Search string (ALT-2)"));
        ret.input_color.
            set_block(Block::default().borders(Borders::ALL).title("Color (ALT-3)"));
        ret.input_distance.
            set_block(Block::default().borders(Borders::ALL).title("Edit distance (ALT-4)"));
        ret
    }
}

#[derive(Debug, PartialEq)]
pub enum UIMode {
    Viewer(SearchPanelState), // memorize search panel state
    SearchPanel(SearchPanelState) 
}
impl UIMode {
   pub fn get_search_panel_state(&self) -> &SearchPanelState {
       match self {
           UIMode::Viewer(state) => state,
           UIMode::SearchPanel(state) => state
       }
   } 
}

#[derive(Debug, PartialEq, Clone, Eq, Hash, Copy)]
pub enum SearchPanelFocus {
    PatternsList,
    InputPattern,
    InputColor,
    InputDistance,
    InputButton
}

#[derive(Debug, PartialEq, Clone, Eq, Hash, Copy)]
pub struct SearchPanelState {
    pub focus: SearchPanelFocus,
    pub patterns_list_selection: Option<usize>
}

impl App<'_> {
    pub fn new(file: String) -> Self {
        let mut reader = fastq::Reader::from_file(file.clone()).expect("Failed to open fastq file");
        let mut record = fastq::Record::new();
        let mut buf_size = 50;
        let mut records = Vec::new();
        reader.read(&mut record).expect("Failed to parse record");
        while !record.is_empty() && buf_size > 0 {
            buf_size -= 1;
            records.push(record.to_owned());
            reader.read(&mut record).expect("Failed to parse record");
        }
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
            records_buf: records,
            line_buf: Vec::new(),
            mode: UIMode::Viewer(SearchPanelState {
                focus: SearchPanelFocus::PatternsList,
                patterns_list_selection: Some(0) 
            }),
            line_num: 0,
            search_panel: SearchPanel::new(&default_search_patterns),
            file,
            reader,
            active_boarder_style: Style::new().red().bold()
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
            UIMode::SearchPanel(SearchPanelState {focus, ..}) => self.highligh_search_panel_focus(*focus),
            any => panic!("Unexpected UI mode {:?}", any)
        };
        self.update();
    }

    pub fn delete_search_pattern(&mut self, index: usize) -> SearchPattern {
        let pattern = self.search_patterns.remove(index);
        self.search_panel = SearchPanel::new(&self.search_patterns);
        match &self.mode {
            UIMode::SearchPanel(SearchPanelState {focus, ..}) => self.highligh_search_panel_focus(*focus),
            any => panic!("Unexpected UI mode {:?}", any)
        };
        self.update();
        return pattern;
    }

    pub fn edit_search_pattern(&mut self, index: usize) {
        let pattern: SearchPattern = self.delete_search_pattern(index);
        self.search_panel.input_pattern = TextArea::new(vec![pattern.search_string]);
        self.search_panel.input_color = TextArea::new(vec![pattern.color.to_string()]);
        self.search_panel.input_distance = TextArea::new(vec![pattern.edit_distance.to_string()]);
        self.search_panel.input_pattern.
            set_block(Block::default().borders(Borders::ALL).title("Search string (ALT-2)"));
        self.search_panel.input_color.
            set_block(Block::default().borders(Borders::ALL).title("Color (ALT-3)"));
        self.search_panel.input_distance.
            set_block(Block::default().borders(Borders::ALL).title("Edit distance (ALT-4)"));

    }

    pub fn toggle_ui_mode(&mut self) {
        match &self.mode {
            UIMode::Viewer(focus) => self.mode = UIMode::SearchPanel(*focus),
            UIMode::SearchPanel(focus) => self.mode = UIMode::Viewer(*focus)
        };
    }

    pub fn scroll(&mut self, num: isize) -> bool {
        if num < 0 && (num.abs() as usize > self.line_num) {
            self.line_num = 0;
            false
        } else {
            self.line_num = (self.line_num as isize + num) as usize;
            true
        }
    }

    fn highligh_search_panel_focus(&mut self, focus: SearchPanelFocus) {
        match self.mode.get_search_panel_state().focus {
            SearchPanelFocus::PatternsList => {
                self.search_panel.patterns_list = self.search_panel.patterns_list
                    .clone()
                    .block(Block::default().
                           title("Search patterns (ALT-1 to switch)")
                           .borders(Borders::ALL))
            },
            SearchPanelFocus::InputPattern => self.search_panel.input_pattern
                .set_block(Block::default().borders(Borders::ALL).title("Search string (ALT-2)")),
            SearchPanelFocus::InputColor => self.search_panel.input_color
                .set_block(Block::default().borders(Borders::ALL).title("Color (ALT-3)")),
            SearchPanelFocus::InputDistance => self.search_panel.input_distance
                .set_block(Block::default().borders(Borders::ALL).title("Edit distance (ALT-4)")),
SearchPanelFocus::InputButton => ()
        };
        match focus {
            SearchPanelFocus::PatternsList => {
                self.search_panel.patterns_list = self.search_panel.patterns_list
                    .clone()
                    .block(Block::default().
                           title("Search patterns (ALT-1 to switch)")
                           .borders(Borders::ALL)
                    .border_style(self.active_boarder_style))
            },
            SearchPanelFocus::InputPattern => self.search_panel.input_pattern
                .set_block(Block::default().borders(Borders::ALL).border_style(self.active_boarder_style).title("Search string")),
            SearchPanelFocus::InputColor => self.search_panel.input_color
                .set_block(Block::default().borders(Borders::ALL).border_style(self.active_boarder_style).title("Color")),
            SearchPanelFocus::InputDistance => self.search_panel.input_distance
                .set_block(Block::default().borders(Borders::ALL).border_style(self.active_boarder_style).title("Edit distance")),
            SearchPanelFocus::InputButton => ()
        };
    }

    pub fn focus_search_panel(&mut self, focus: SearchPanelFocus) {
        self.highligh_search_panel_focus(focus);
        self.mode = UIMode::SearchPanel(SearchPanelState { focus, patterns_list_selection: self.mode.get_search_panel_state().patterns_list_selection });
    }

    pub fn cycle_patterns_list(&mut self, reverse: bool) {
        if self.search_patterns.len() == 0 {
            self.mode = UIMode::SearchPanel(SearchPanelState { focus: self.mode.get_search_panel_state().focus, patterns_list_selection: None });
            return;
        }
        let new_selection = match self.mode.get_search_panel_state().patterns_list_selection {
            Some(selection) => {
                if reverse {
                    Some(selection.checked_sub(1).unwrap_or(self.search_patterns.len() - 1))
                } else {
                    if selection == self.search_patterns.len() - 1 {
                        Some(0)
                    } else {
                        Some(selection + 1)
                    }
                }
            },
            None => {
                if reverse {
                    Some(self.search_patterns.len() - 1)
                } else {
                    Some(0)
                }
                }  
        };
        self.mode = UIMode::SearchPanel(SearchPanelState { focus: self.mode.get_search_panel_state().focus, patterns_list_selection: new_selection });
    }

    pub fn message(&mut self, msg: String) {
        self.search_panel.input_button = Paragraph::new("ALT-5 to add pattern\n".to_string() + &*msg).
            alignment(Alignment::Center)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL));
    }

    pub fn update(&mut self) {
        let mut result: Vec<Line> = Vec::new();
        for record in &self.records_buf {
            let seq = String::from_utf8_lossy(record.seq()).to_string();

            let mut matches: Vec<(IntervalSet<usize>, Color)> = Vec::new();
            for x in &self.search_patterns {
                let mut myers = Myers::<u64>::new(x.search_string.clone().into_bytes());
                matches.push((myers
                                  .find_all(record.seq(), x.edit_distance)
                                  .map(|(a, b, _)| (a, b - 1))
                                  .collect::<Vec<(usize, usize)>>()
                                  .to_interval_set(), x.color));
            }
            result.push(record.id().to_string().into());
            result.push(highlight_matches(&matches, seq, Color::Gray));
        }
        self.line_buf = result;
    }
}
