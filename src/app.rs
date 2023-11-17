use crate::read_stylizing::highlight_matches;
use std::str::FromStr;
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
        .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
        .highlight_symbol(">del ")
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
    Viewer,
    SearchPanel(SearchPanelFocus)
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub enum SearchPanelFocus {
    PatternsList,
    InputPattern,
    InputColor,
    InputDistance,
    InputButton
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
            mode: UIMode::Viewer,
            line_num: 0,
            search_panel: SearchPanel::new(&default_search_patterns),
            file,
            reader,
            active_boarder_style: Style::new().red().bold()
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
        self.search_panel.patterns_list = search_patterns_to_list(&self.search_patterns);
        self.update();
    }

    pub fn toggle_ui_mode(&mut self) {
        if self.mode == UIMode::Viewer {
            self.mode = UIMode::SearchPanel(SearchPanelFocus::PatternsList)
        } else {
            self.mode = UIMode::Viewer
        }
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

    pub fn focue_search_panel(&mut self, focus: SearchPanelFocus) {
        if let UIMode::SearchPanel(prev_focus) = &self.mode {
        match prev_focus{
            SearchPanelFocus::PatternsList => {
                self.search_panel.patterns_list = self.search_panel.patterns_list
                    .clone()
                    .block(Block::default().
                           title("Search patterns (ALT-1)")
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
        };
        match focus {
            SearchPanelFocus::PatternsList => {
                self.search_panel.patterns_list = self.search_panel.patterns_list
                    .clone()
                    .block(Block::default().
                           title("Search patterns")
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
        self.mode = UIMode::SearchPanel(focus);
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
            for (x) in &self.search_patterns {
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
