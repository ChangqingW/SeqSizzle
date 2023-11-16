use crate::read_stylizing::highlight_matches;
use std::str::FromStr;
use bio::io::fastq;
use bio::io::fastq::FastqRead;
use bio::io::fastq::Reader;
use bio::pattern_matching::myers::Myers;
use ratatui::prelude::{Line, Color, Span, Style, Modifier};
use ratatui::widgets::{List, ListItem, Block, Borders};
use interval::interval_set::ToIntervalSet;
use interval::IntervalSet;
use tui_textarea::TextArea;

#[derive(Debug)]
pub struct App<'a> {
    pub quit: bool,
    pub search_patterns: Vec<(String, String, u8)>,
    pub records_buf: Vec<fastq::Record>,
    pub line_buf: Vec<Line<'a>>,
    pub mode: UIMode,
    pub line_num: usize,
    pub search_panel: SearchPanel<'a>,
    file: String,
    reader: Reader<std::io::BufReader<std::fs::File>>, // buf_size
}

#[derive(Debug)]
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
    pub input_distance: TextArea<'a>
//    input_button: dyn Widget
}

fn search_patterns_to_list<'a>(search_patterns: &[(String, String, u8)]) -> List<'a> {
    List::new(search_patterns.iter()
        .map(|(pattern, color, distance)| {
            ListItem::new(Line::from(vec![
                Span::from(pattern.clone()),
                Span::from(", color: "),
                Span::styled(color.clone(), Style::new().fg(Color::from_str(color).unwrap())),
                Span::from(format!(", edit-distance: {}", distance))
            ]))
        })
        .collect::<Vec<ListItem>>())
        .block(Block::default().title("Search patterns").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
        .highlight_symbol(">del ")
}
impl SearchPanel<'_> {
    pub fn new(search_patterns: &[(String, String, u8)]) -> Self {
        let mut ret = Self {
            patterns_list: search_patterns_to_list(search_patterns),
            input_pattern: TextArea::default(),
            input_color: TextArea::default(),
            input_distance: TextArea::default(),
        };
        ret.input_pattern.
            set_block(Block::default().borders(Borders::ALL).title("Search string"));
        ret.input_color.
            set_block(Block::default().borders(Borders::ALL).title("Color"));
        ret.input_distance.
            set_block(Block::default().borders(Borders::ALL).title("Edit distance"));
        ret
    }
}

#[derive(Debug, PartialEq)]
pub enum UIMode {
    Viewer,
    SearchPanel(SearchPanelFocus)
}

#[derive(Debug, PartialEq)]
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
            ("CTACACGACGCTCTTCCGATCT".to_string(), "#00FF00".to_string(), 3),
            ("AGATCGGAAGAGCGTCGTGTAG".to_string(), "#FF0000".to_string(), 3),
            ("TTTTTTTTTTTT".to_string(), "#00FF00".to_string(), 0),
            ("AAAAAAAAAAAA".to_string(), "#FF0000".to_string(), 0),
            ("TCTTCTTTC".to_string(), "#FFC0CB".to_string(), 0),
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
        };
        instance.update();
        instance
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn set_search_patterns(&mut self, search_patterns: Vec<(String, String, u8)>) {
        self.search_patterns = search_patterns;
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

    pub fn update(&mut self) {
        let mut result: Vec<Line> = Vec::new();
        for record in &self.records_buf {
            let seq = String::from_utf8_lossy(record.seq()).to_string();

            let mut matches: Vec<(IntervalSet<usize>, &str)> = Vec::new();
            for (pattern, col, dist) in &self.search_patterns {
                let mut myers = Myers::<u64>::new(pattern.clone().into_bytes());
                matches.push((myers
                                  .find_all(record.seq(), *dist)
                                  .map(|(a, b, _)| (a, b - 1))
                                  .collect::<Vec<(usize, usize)>>()
                                  .to_interval_set(), col));
            }
            result.push(record.id().to_string().into());
            result.push(highlight_matches(&matches, seq, "#0000FF"));
        }
        self.line_buf = result;
    }
}
