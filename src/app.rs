use crate::read_stylizing::{highligh_matches, merge_intervals};
use bio::alignment::Alignment;
use bio::io::fastq;
use bio::io::fastq::Reader;
use bio::io::fastq::FastqRead;
use bio::pattern_matching::myers::Myers;
use ratatui::prelude::Line;

#[derive(Debug)]
pub struct App {
    pub quit: bool,
    pub search_patterns: Vec<(String, String)>,
    pub records_buf: Vec<fastq::Record>,
    file: String,
    reader: Reader<std::io::BufReader<std::fs::File>> // buf_size
}

impl App {
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
        App {
            reader: reader,
            quit: false,
            search_patterns: vec![("AGATCGGAAGAGCGTCGTGTAGAA".to_string(), "00FF00".to_string())],
            records_buf: records,
            file: file,
        }
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn set_search_patterns(&mut self, search_patterns: Vec<(String, String)>) {
        self.search_patterns = search_patterns;
    }

    pub fn update(&self) -> Vec<Line> {
        let mut result: Vec<Line> = Vec::new();
        let record = &self.records_buf[0];
        let seq = String::from_utf8_lossy(record.seq()).to_string();

        let mut myers = Myers::<u64>::new(b"AGATCGGAAGAGCGTCGTGTAGAA");
        let _aln = Alignment::default();
        let matches: Vec<(usize, usize)> = merge_intervals(
            myers
                .find_all(record.seq(), 3)
                .map(|(a, b, _)| (a, b))
                .collect(),
        );

        result.push(record.id().to_string().into());
        result.push(highligh_matches(matches, seq));
        result
    }
}
