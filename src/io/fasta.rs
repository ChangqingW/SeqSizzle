use crate::io::fastx::FastxReader;
use bio::io::{fasta, fastq};
use std::collections::VecDeque;
use std::env::temp_dir;
use std::fmt::Debug;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// skip n records in a fasta file
/// reuturns the ID of the next record
fn skip_n_records<R: Read>(
    buf_reader: &mut BufReader<R>,
    n: usize,
) -> Result<String, std::io::Error> {
    let mut line = String::new();
    let mut skipped = 0;
    while skipped < n {
        match buf_reader.read_line(&mut line) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("skip_n_records EOF reached after {} lines", n),
                ));
            }
            Ok(_) => {
                if line.starts_with('>') {
                    skipped += 1;
                }
            }
            Err(e) => return Err(e),
        }
    }
    return Ok(line.trim_end()[1..].to_string());
}

#[cfg(not(debug_assertions))]
static RECORD_BUF_SIZE: usize = 1024;
#[cfg(not(debug_assertions))]
static READER_BUF_SIZE: usize = RECORD_BUF_SIZE * 4 * 1024; // 4MB

#[cfg(debug_assertions)]
static RECORD_BUF_SIZE: usize = 4;
#[cfg(debug_assertions)]
static READER_BUF_SIZE: usize = RECORD_BUF_SIZE * 4 * 300;

#[derive(Debug)]
pub struct FastaReader<R: Read + Seek> {
    buf_reader: BufReader<R>,
    records_buffer: VecDeque<fastq::Record>,
    offset: usize, // offset of the first record in the buffer
    total_records: Option<usize>,
    next_id: String,
    // need to reach the next ID field to know the current record ends
    // hence we need to store it for the next record
}

impl<R: Read + Seek + Debug> FastaReader<R> {
    pub fn new(mut reader: R) -> Self {
        assert!(
            reader.stream_position().unwrap() == 0,
            "reader not at the start of the file"
        );
        // read until the first record
        let mut buf_reader = BufReader::with_capacity(READER_BUF_SIZE, reader);
        let mut line = String::new();
        while buf_reader.read_line(&mut line).unwrap() > 0 {
            if line.starts_with('>') {
                break;
            }
            line.clear();
        }

        FastaReader {
            buf_reader,
            records_buffer: VecDeque::with_capacity(RECORD_BUF_SIZE + 1),
            offset: 0,
            total_records: None,
            next_id: line.trim_end()[1..].to_string(),
        }
    }

    fn pop_front(&mut self) {
        if self.records_buffer.pop_front().is_some() {
            self.offset += 1;
        } else {
            panic!("pop_front called on empty buffer");
        }
    }
}

impl<R: Read + Seek + Debug> FastxReader<R> for FastaReader<R> {
    fn fill_buffer(&mut self) -> Result<(), std::io::Error> {
        for _ in 0..RECORD_BUF_SIZE {
            let mut line = String::new();
            loop {
                let mut seq = String::new();
                match self.buf_reader.read_line(&mut line) {
                    Ok(0) => {
                        self.total_records = Some(self.offset + self.records_buffer.len());
                        return Ok(());
                    }
                    Err(e) => return Err(e),
                    Ok(_) => {
                        if line.starts_with('>') {
                            self.records_buffer.push_back(fastq::Record::with_attrs(
                                &self.next_id,
                                None,
                                &seq.as_bytes(),
                                &"I".repeat(seq.len()).as_bytes(),
                            ));
                            self.next_id = line.trim_end()[1..].to_string();
                            break;
                        } else {
                            seq.push_str(&line.trim_end());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn rewind(&mut self) -> Result<(), std::io::Error> {
        todo!()
    }
    fn get_index(&mut self, index: usize) -> Result<Option<fastq::Record>, std::io::Error> {
        todo!()
    }
}
