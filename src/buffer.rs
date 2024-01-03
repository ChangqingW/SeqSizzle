use crate::io::bifastq::BidirectionalFastqReader;
use crate::app::SearchPattern;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use bio::io::fastq;
use ratatui::{
    prelude::Line,
    widgets::{Paragraph, Wrap},
};

#[cfg(debug_assertions)]
const RECORDS_BUF_SIZE: usize = 4; // Need to be a multiple of 4

#[cfg(not(debug_assertions))]
const RECORDS_BUF_SIZE: usize = 12; 

#[derive(Debug, Clone)]
pub struct Read<'a> {
    pub read: fastq::Record,
    // highlighted lines
    // Lines cannot be optinally rendered, because rendering require
    // reference to search patterns, which are mutable
    // hence all lines are highlighted and re-hlighlighed on every
    // search pattern update
    pub lines: Vec<Line<'a>>
}

impl<'a> Read<'a> {
    pub fn new(read: fastq::Record) -> Self {
        Self {
            read,
            lines: vec![Line::from("Line unrendered!")],
        }
    }

    pub fn calculate_height(&self, width: u16) -> u16 {
        assert_ne!(width, 0);
        self.lines
            .iter()
            .map(|line| u16::try_from(line.width().div_ceil(width as usize)).unwrap())
            .sum()
    }

}

#[derive(Debug)]
pub struct ReadBuffer<'a> {
    pub reads: VecDeque<Read<'a>>, // a VecDeque allows for bidirectional insertion
    file: PathBuf,
    reader: BidirectionalFastqReader<File>,
    pub buf_bounded: (bool, bool), // buffer reached start / end of file
    offset: i16, // every time we discard reads to save memory, we adjust offset so it always represents the global read idx in the file
    max_index: Option<usize>,
}

// TODO: No idea how the lifetime annotations work
pub struct ReadIterator<'a, 'b> {
    inner: &'a mut ReadBuffer<'b>,
    index: usize,
}
impl <'a, 'b>Iterator for ReadIterator<'a, 'b> {
    type Item = Read<'b>;

    fn next(&mut self) -> Option<Self::Item> {
        let read = self.inner.get_index(self.index);
        self.index += 1;
        read.cloned()
    }
}


impl<'a> ReadBuffer<'a> {
    pub fn new(file: String) -> Self {
        let mut reader = BidirectionalFastqReader::from_path(&Path::new(&file));

        return Self {
            // perform RECORDS_BUF_SIZE initial reads to populate 'reads'
            reads: reader
                .next_n(RECORDS_BUF_SIZE)
                .expect("Failed to parse record")
                .into_iter()
                .map(|x| Read::new(x))
                .collect(),
            file: Path::new(&file).to_path_buf(),
            reader,
            buf_bounded: (true, false),
            offset: 0,
            max_index: None,
        };
    }

    // we create a custom get_index function which is able to dynamically fetch reads if required
    pub fn get_index(&mut self, index: usize) -> Option<&Read<'a>> {
        self.ensure_reserved_space(index);

        if let Some(max_index) = self.max_index {
            if index > max_index {
                return None;
            }
        }

        let vec_idx: usize = (index as i16 - self.offset).try_into().unwrap();

        if self.max_index.is_some() && vec_idx > self.max_index.unwrap() {
            None
        } else {
            Some(self.reads.get_mut(vec_idx).expect("Index should exist"))
        }
    }

    // this function will always ensure that relevant buffers have been established
    fn ensure_reserved_space(&mut self, index: usize) {
        let vec_idx = index as i16 - self.offset;

        if let Some(max_index) = self.max_index {
            if index > max_index {
                return;
            }
        }

        if vec_idx < 0 {
            self.buffer_backward();
        } else if vec_idx as usize >= self.reads.len() {
            self.buffer_forward();
        } else {
            return;
        }

        // if we've had to buffer, check if we need to buffer again
        self.ensure_reserved_space(index);
    }

    // buffer forward i.e. load reads with a higher index
    fn buffer_forward(&mut self) {
        let new_records = self.reader.next_n(RECORDS_BUF_SIZE / 4).unwrap();

        let len = new_records.len();
        if len < RECORDS_BUF_SIZE / 4 {
            self.max_index = Some(len + self.offset as usize);
        }

        // we both add new reads to one end, and remove reads from the other end
        for record in new_records {
            if self.reads.len() >= RECORDS_BUF_SIZE {
                // we increment the offset by one, as the new front of queue is
                // has one higher actual index
                self.offset += 1;
                self.reads.pop_front();
            }
            self.reads.push_back(Read::new(record));
        }
    }

    // buffer backwards
    fn buffer_backward(&mut self) {
        let new_records = self
            .reader
            .prev_n(self.reads.len() + RECORDS_BUF_SIZE / 4)
            //      ^^^                ^^^
            //      existing vec len   len of extra buffer on the end
            .unwrap();

        // TODO: fix scrolling when buffer forwarded skipping lines
        // rewind reader head?

        for record in new_records {
            if self.reads.len() >= RECORDS_BUF_SIZE {
                self.reads.pop_back();
            }
            self.reads.push_front(Read::new(record));
            self.offset -= 1;
        }
    }

    pub fn iter_from(&mut self, index: usize) -> ReadIterator<'_, 'a> {
        ReadIterator {
            inner: self,
            index,
        }
    }
}
