use crate::read_stylizing::{highligh_matches, merge_intervals};
use bio::alignment::Alignment;
use bio::io::fastq;
use bio::io::fastq::FastqRead;
use bio::io::fastq::Reader;
use bio::pattern_matching::myers::Myers;
use ratatui::prelude::Line;

#[derive(Debug)]
pub struct App {
    pub quit: bool,
    pub search_patterns: Vec<(String, String)>,
    pub records_buf: Vec<fastq::Record>,
    file: String,
    reader: Reader<std::io::BufReader<std::fs::File>>, // buf_size
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
            reader,
            quit: false,
            search_patterns: vec![
                ("AGATCGGAAGAGCGTCGTGTAGAA".to_string(), "#00FF00".to_string()),
                ("AACGCAGAGGAA".to_string(), "#FF0000".to_string()),
                ("TCTTCCGA".to_string(), "#ffff00".to_string()),
            ],
            records_buf: records,
            file,
        }
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn set_search_patterns(&mut self, search_patterns: Vec<(String, String)>) {
         self.search_patterns = search_patterns;
    }

    pub fn update<'a>(&self) -> Vec<Line<'a>> {
        let mut result: Vec<Line> = Vec::new();
        for record in &self.records_buf {
            let seq = String::from_utf8_lossy(record.seq()).to_string();

            let mut matches: Vec<(Vec<(usize, usize)>, &str)> = Vec::new();
            for (pattern, col) in &self.search_patterns {
                let mut myers_1 = Myers::<u64>::new(pattern.clone().into_bytes());
                let _aln = Alignment::default();
                let matches_1: Vec<(usize, usize)> = merge_intervals(
                    &myers_1
                        .find_all(record.seq(), 2)
                        .map(|(a, b, _)| (a, b))
                        .collect::<Vec<(usize, usize)>>(),
                );
                matches.push((matches_1, col));
            }
            result.push(record.id().to_string().into());
            result.push(highligh_matches(&matches, seq, "#0000FF"));
        }
        result
    }
}
