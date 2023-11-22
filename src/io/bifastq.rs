use bio::io::fastq;
use std::env::temp_dir;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::path::PathBuf;
use uuid::Uuid;

pub struct BidirectionalFastqReader<File> {
    buf_reader: BufReader<File>,
}

impl<R: Sized + Read + Seek> BidirectionalFastqReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            buf_reader: BufReader::new(reader),
        }
    }

    fn parse_record(&mut self) -> Result<Option<fastq::Record>, std::io::Error> {
        let mut id = String::new();
        let mut seq = String::new();
        let mut qual = String::new();

        let status: (
            Result<usize, std::io::Error>,
            Result<usize, std::io::Error>,
            Result<usize, std::io::Error>,
            Result<usize, std::io::Error>,
        ) = (
            self.buf_reader.read_line(&mut id),
            self.buf_reader.read_line(&mut seq),
            self.buf_reader.read_line(&mut String::new()), // skip '+'
            self.buf_reader.read_line(&mut qual),
        );
        match status {
            (Ok(0), Ok(0), Ok(0), Ok(0)) => Ok(None), // EOF reached
            (Ok(_), Ok(_), Ok(_), Ok(_)) => {
                // id starts with '@'
                if id.starts_with('@') {
                    Ok(Some(fastq::Record::with_attrs(
                        &id.trim_end()[1..],
                        None,
                        &seq.trim_end().as_bytes(),
                        &qual.trim_end().as_bytes(),
                    )))
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("ID field does not start with '@': {}{}{}", id, seq, qual),
                    ))
                }
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Error while parsing lines: {}\n{}\n{}\n", id, seq, qual),
            )),
        }
    }

    fn seek_to_prev_record(&mut self) -> bool {
        for i in 0..5 {
            loop {
                match self.buf_reader.seek_relative(-1) {
                    Err(e) => {
                        match i {
                            0 => return false, // already at start of file
                            4 => return true,  // start of file reached
                            _ => panic!("Error while seeking backwards: {:?}", e),
                        }
                    }

                    Ok(_) => {
                        let mut buf = [0];
                        match self.buf_reader.read(&mut buf) {
                            Ok(0) => panic!("EOF reached while seeking backwards"),
                            Ok(_) if buf[0] == b'\n' => {
                                let _ = self.buf_reader.seek_relative(-1);
                                break;
                            }
                            Ok(_) => {
                                let _ = self.buf_reader.seek_relative(-1);
                                continue;
                            }
                            Err(e) => {
                                panic!("Error while reading: {:?}", e);
                            }
                        }
                    }
                }
            }
        }

        self.buf_reader
            .seek_relative(1)
            .expect("Error while seeking forwards");
        true
    }

    pub fn rewind_n(&mut self, n: usize) -> Result<usize, std::io::Error> {
        for i in 0..n {
            if !self.seek_to_prev_record() {
                return Ok(i);
            }
        }

        Ok(n)
    }

    pub fn next(&mut self) -> Result<Option<fastq::Record>, std::io::Error> {
        self.parse_record()
    }

    pub fn prev(&mut self) -> Result<Option<fastq::Record>, std::io::Error> {
        self.seek_to_prev_record();
        self.parse_record()
    }

    pub fn next_n(&mut self, n: usize) -> Result<Vec<fastq::Record>, std::io::Error> {
        let mut records = Vec::with_capacity(n);
        for _ in 0..n {
            match self.next()? {
                Some(record) => records.push(record),
                None => break,
            }
        }
        Ok(records)
    }

    pub fn prev_n(&mut self, n: usize) -> Result<Vec<fastq::Record>, std::io::Error> {
        for i in 0..n {
            if !self.seek_to_prev_record() {
                let mut ret: Vec<fastq::Record> = self.next_n(i)?;
                ret.reverse();
                return Ok(ret);
            }
        }
        let mut ret: Vec<fastq::Record> = self.next_n(n)?;
        ret.reverse();
        Ok(ret)
    }
}

#[allow(dead_code)] // It IS used in the tests ffs
fn setup_test() -> (PathBuf, BidirectionalFastqReader<File>, Vec<fastq::Record>) {
    let mut file_name = temp_dir();
    file_name.push(format!("{}.fastq", Uuid::new_v4()));
    let mut file = File::create(file_name.clone()).unwrap();
    file.write_all(b"@id1\nAAAA\n+\nIIII\n@id2\nTTTT\n+\nIIII\n@id3\nCCCC\n+\nIIII\n")
        .unwrap();
    file.sync_all().unwrap();
    let reader = BidirectionalFastqReader::new(File::open(file_name.clone()).unwrap());
    let records: Vec<fastq::Record> =
        fastq::Reader::new(File::open(file_name.clone()).unwrap())
            .records()
            .map(|r| r.unwrap())
            .collect();

    (file_name, reader, records)
}

#[allow(dead_code)]
fn cleanup_test(file_name: PathBuf) {
    match std::fs::remove_file(file_name.clone()) {
        Ok(_) => (),
        Err(e) => panic!("Error removing test file {}: {:?}", file_name.to_string_lossy(), e),
    }
}

#[test]
fn test_parse_record() {
    let (file_name, mut reader, records) = setup_test();
    assert_eq!(reader.parse_record().unwrap().unwrap(), records[0]);
    assert_eq!(reader.parse_record().unwrap().unwrap(), records[1]);
    assert_eq!(reader.parse_record().unwrap().unwrap(), records[2]);
    assert_eq!(reader.parse_record().unwrap(), None);
    cleanup_test(file_name);
}

#[test]
fn test_seek_to_prev_record() {
    let (file_name, mut reader, records) = setup_test();
    assert_eq!(reader.parse_record().unwrap().unwrap(), records[0]);
    assert_eq!(reader.seek_to_prev_record(), true);
    assert_eq!(reader.parse_record().unwrap().unwrap(), records[0]);
    assert_eq!(reader.seek_to_prev_record(), true);
    assert_eq!(reader.seek_to_prev_record(), false); // start of file reached

    assert_eq!(reader.parse_record().unwrap().unwrap(), records[0]);
    assert_eq!(reader.parse_record().unwrap().unwrap(), records[1]);
    assert_eq!(reader.seek_to_prev_record(), true);
    assert_eq!(reader.parse_record().unwrap().unwrap(), records[1]);
    cleanup_test(file_name);
}

#[test]
fn test_prev_n() {
    let (file_name, mut reader, records) = setup_test();
    assert_eq!(reader.next_n(3).unwrap(), records);
    assert_eq!(
        reader.prev_n(3).unwrap(),
        records.iter().rev().cloned().collect::<Vec<_>>()
    );
    assert_eq!(
        reader.prev_n(4).unwrap(),
        records.iter().rev().cloned().collect::<Vec<_>>()
    );
    cleanup_test(file_name);
}

#[test]
fn test_rewind_n() {
    let (file_name, mut reader, records) = setup_test();
    assert_eq!(reader.next_n(3).unwrap(), records);
    assert_eq!(reader.rewind_n(4).unwrap(), 3);
    assert_eq!(reader.next_n(3).unwrap(), records);
    assert_eq!(reader.rewind_n(2).unwrap(), 2);
    assert_eq!(reader.next_n(3).unwrap(), records[1..]);
    cleanup_test(file_name);
}
