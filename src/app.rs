use crate::read_stylizing::highlight_matches;

use bio::io::fastq;
use bio::io::fastq::FastqRead;
use bio::io::fastq::Reader;
use bio::pattern_matching::myers::Myers;
use ratatui::prelude::Line;
use interval::interval_set::ToIntervalSet;
use interval::IntervalSet;

#[derive(Debug)]
pub struct App<'a> {
    pub quit: bool,
    pub search_patterns: Vec<(String, String, u8)>,
    pub records_buf: Vec<fastq::Record>,
    pub line_buf: Vec<Line<'a>>,
    pub mode: UIMode,
    pub line_num: usize,
    file: String,
    reader: Reader<std::io::BufReader<std::fs::File>>, // buf_size
}

#[derive(Debug)]
pub enum UIMode {
    Viewer,
    SearchPopup
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
        let mut instance = App {
            quit: false,
            search_patterns: vec![
                ("CTACACGACGCTCTTCCGATCT".to_string(), "#00FF00".to_string(), 3),
                ("AGATCGGAAGAGCGTCGTGTAG".to_string(), "#FF0000".to_string(), 3),
                ("TTTTTTTTTTTT".to_string(), "#00FF00".to_string(), 0),
                ("AAAAAAAAAAAA".to_string(), "#FF0000".to_string(), 0),
                ("TCTTCTTTC".to_string(), "#FFC0CB".to_string(), 0),
            ],
            records_buf: records,
            line_buf: Vec::new(),
            mode: UIMode::Viewer,
            line_num: 0,
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
