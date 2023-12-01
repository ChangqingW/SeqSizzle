use bio::io::fastq;
use std::collections::VecDeque;
use std::env::temp_dir;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Parse a fastq record from a BufReader
///
/// Assumes the file pointer is at the start of a record
/// Reads 4 lines from the BufReader and parses them into a fastq::Record
/// Returns None if EOF is reached, Error if the lines are not valid fastq
fn parse_record<R: Read>(
    buf_reader: &mut BufReader<R>,
) -> Result<Option<fastq::Record>, std::io::Error> {
    let mut id = String::new();
    let mut seq = String::new();
    let mut qual = String::new();

    let status: (
        Result<usize, std::io::Error>,
        Result<usize, std::io::Error>,
        Result<usize, std::io::Error>,
        Result<usize, std::io::Error>,
    ) = (
        buf_reader.read_line(&mut id),
        buf_reader.read_line(&mut seq),
        buf_reader.read_line(&mut String::new()), // skip '+'
        buf_reader.read_line(&mut qual),
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

/// Try to parse a fastq record from a BufReader
///
/// Try reading 7 lines from the BufReader and
/// workout the start of a fastq record
/// calls next if the start of a record is found
fn try_next<R: Read + Seek>(
    buf_reader: &mut BufReader<R>,
) -> Result<BufferedRecord, std::io::Error> {
    let mut lines = Vec::with_capacity(8);
    for i in 0..7 {
        let mut line = String::new();
        let pos = buf_reader.stream_position()?;
        match buf_reader.read_line(&mut line) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("try_parse_record EOF reached after {} lines", i),
                ));
            }
            Ok(_) => lines.push((line, pos)),
            Err(e) => return Err(e),
        }
    }
    for i in 0..4 {
        if lines[i].0.starts_with('@')
            && lines[i + 2].0.starts_with('+')
            && lines[i + 1].0.len() == lines[i + 3].0.len()
        {
            buf_reader.seek(std::io::SeekFrom::Start(lines[i].1))?;
            return Ok(next(buf_reader).unwrap().unwrap());
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "try_parse_record could not find a valid record",
    ))
}

/// Parse a fastq record from a BufReader
/// and return the record and the file position
fn next<R: Read + Seek>(
    buf_reader: &mut BufReader<R>,
) -> Result<Option<BufferedRecord>, std::io::Error> {
    let pos = buf_reader.stream_position()?;
    let rec: Option<fastq::Record> = parse_record(buf_reader)?;
    if rec.is_some() {
        Ok(Some(BufferedRecord {
            record: rec.unwrap(),
            file_position: pos,
        }))
    } else {
        Ok(None)
    }
}

/// Read records from a BufReader until the given position is reached
fn read_to_pos<R: Read + Seek>(
    buf_reader: &mut BufReader<R>,
    pos: u64,
) -> Result<VecDeque<BufferedRecord>, std::io::Error> {
    let mut buff: VecDeque<BufferedRecord> = VecDeque::with_capacity(RECORD_BUF_SIZE + 1);
    buff.push_back(try_next(buf_reader)?);
    loop {
        let res: Option<BufferedRecord> = next(buf_reader)?;
        match res {
            Some(rec) => {
                let current_pos = buf_reader.stream_position()?;
                if current_pos == pos {
                    buff.push_back(rec);
                    break;
                } else if current_pos > pos {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "read_to_pos exceeded given pos",
                    ));
                } else {
                    buff.push_back(rec);
                    if buff.len() >= RECORD_BUF_SIZE {
                        buff.pop_front();
                    }
                }
            }
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid data found in read_to_pos",
                ));
            }
        }
    }
    Ok(buff)
}

#[cfg(not(debug_assertions))]
static RECORD_BUF_SIZE: usize = 1024 * 1024; // 1MB
#[cfg(not(debug_assertions))]
static READER_BUF_SIZE: usize = 2000;

#[cfg(debug_assertions)]
static RECORD_BUF_SIZE: usize = 20;
#[cfg(debug_assertions)]
static READER_BUF_SIZE: usize = RECORD_BUF_SIZE * 4 * 300;

#[derive(Debug, Clone)]
struct BufferedRecord {
    record: fastq::Record,
    file_position: u64,
}

#[derive(Debug)]
pub struct BidirectionalFastqReader<R: Read + Seek> {
    buf_reader: BufReader<R>,
    backwards_records_buffer: VecDeque<BufferedRecord>,
}

// Constructor for File
impl BidirectionalFastqReader<File> {
    pub fn from_path(path: &Path) -> Self {
        Self {
            buf_reader: BufReader::with_capacity(
                READER_BUF_SIZE,
                match File::open(path) {
                    Ok(file) => file,
                    Err(e) => panic!("Error opening file '{}': {:?}", path.to_string_lossy(), e),
                },
            ),
            backwards_records_buffer: VecDeque::with_capacity(RECORD_BUF_SIZE + 1),
        }
    }
}

// Generic methods
impl<R: Read + Seek> BidirectionalFastqReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            buf_reader: BufReader::with_capacity(READER_BUF_SIZE, reader),
            backwards_records_buffer: VecDeque::with_capacity(RECORD_BUF_SIZE + 1),
        }
    }

    pub fn next(&mut self) -> Result<Option<fastq::Record>, std::io::Error> {
        let res: Option<BufferedRecord> = next(&mut self.buf_reader)?;
        if res.is_none() {
            Ok(None)
        } else {
            self.backwards_records_buffer
                .push_back(res.clone().unwrap());
            if self.backwards_records_buffer.len() > RECORD_BUF_SIZE {
                self.backwards_records_buffer.pop_front();
            }
            Ok(Some(res.unwrap().record))
        }
    }

    /// returns the previous record and moves the file pointer to the start of
    /// the returned record
    pub fn prev(&mut self) -> Result<Option<fastq::Record>, std::io::Error> {
        if self.buf_reader.stream_position()? == 0 {
            return Ok(None);
        }
        let poped = self.backwards_records_buffer.pop_back();
        match poped {
            Some(record) => {
                self.buf_reader
                    .seek(std::io::SeekFrom::Start(record.file_position))?;
                Ok(Some(record.record))
            }
            None => {
                // Seek backwards READER_BUF_SIZE bytes, or to the start of the file
                // usize(u32/u64) to u64
                let current_pos = self.buf_reader.stream_position()?;
                let seek = if current_pos > READER_BUF_SIZE as u64 {
                    std::io::SeekFrom::Current(-(READER_BUF_SIZE as i64))
                } else {
                    std::io::SeekFrom::Start(0)
                };
                self.buf_reader.seek(seek)?;
                self.backwards_records_buffer = read_to_pos(&mut self.buf_reader, current_pos)?;
                match self.backwards_records_buffer.pop_back() {
                    Some(record) => {
                        self.buf_reader
                            .seek(std::io::SeekFrom::Start(record.file_position))?;
                        Ok(Some(record.record))
                    }
                    None => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "prev() could not find a valid record before current position",
                        ))
                    }
                }
            }
        }
    }

    pub fn next_n(&mut self, n: usize) -> Result<Vec<fastq::Record>, std::io::Error> {
        let mut records: Vec<fastq::Record> = Vec::with_capacity(n);
        for _ in 0..n {
            match self.next()? {
                Some(record) => records.push(record),
                None => break,
            }
        }
        Ok(records)
    }

    pub fn prev_n(&mut self, n: usize) -> Result<Vec<fastq::Record>, std::io::Error> {
        if n > RECORD_BUF_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "prev_n() can only return up to {} records, got {}",
                    RECORD_BUF_SIZE, n
                ),
            ));
        }
        let mut records: Vec<fastq::Record> = Vec::with_capacity(n);
        for _ in 0..n {
            match self.prev()? {
                Some(record) => records.push(record),
                None => break,
            }
        }
        records.reverse();
        Ok(records)
    }

    pub fn rewind_to_start(&mut self) -> Result<(), std::io::Error> {
        self.buf_reader.seek(std::io::SeekFrom::Start(0))?;
        self.backwards_records_buffer.clear();
        Ok(())
    }
}

#[allow(dead_code)]
fn setup_test() -> (PathBuf, BidirectionalFastqReader<File>, Vec<fastq::Record>) {
    let mut file_name = temp_dir();
    file_name.push(format!("{}.fastq", Uuid::new_v4()));
    let mut file = File::create(file_name.clone()).unwrap();
    file.write_all(
        b"@id1\nAAAA\n+\nIIII\n\
                     @id2\nTTTT\n+\nIIII\n\
                     @id3\nCCCC\n+\nIIII\n\
                     @id4\nGGGG\n+\nIIII\n\
                     @id5\nACCC\n+\nIIII\n\
                     @id6\nCACC\n+\nIIII\n\
                     @id7\nCCAC\n+\nIIII\n\
                     @id8\nCCCA\n+\nIIII\n\
                     @id9\nTCCC\n+\nIIII\n\
                     @id10\nCTCC\n+\nIIII\n",
    )
    .unwrap();
    file.sync_all().unwrap();
    let reader = BidirectionalFastqReader::new(File::open(file_name.clone()).unwrap());
    let records: Vec<fastq::Record> = fastq::Reader::new(File::open(file_name.clone()).unwrap())
        .records()
        .map(|r| r.unwrap())
        .collect();

    (file_name, reader, records)
}

#[allow(dead_code)]
fn cleanup_test(file_name: PathBuf) {
    match std::fs::remove_file(file_name.clone()) {
        Ok(_) => (),
        Err(e) => panic!(
            "Error removing test file {}: {:?}",
            file_name.to_string_lossy(),
            e
        ),
    }
}

#[test]
fn test_parse_record() {
    let (file_name, mut reader, records) = setup_test();
    for i in 0..10 {
        assert_eq!(reader.next().unwrap().unwrap(), records[i]);
    }
    assert_eq!(reader.next().unwrap(), None);
    cleanup_test(file_name);
}

#[test]
fn test_prev_n_1() {
    let (file_name, mut reader, records) = setup_test();
    assert_eq!(reader.next_n(20).unwrap(), records);
    assert_eq!(reader.prev_n(4).unwrap(), records[6..]);
    assert_eq!(reader.prev_n(4).unwrap(), records[2..6]);
    assert_eq!(reader.prev_n(4).unwrap(), records[..2]);
    assert_eq!(reader.next_n(2).unwrap(), records[..2]);
    cleanup_test(file_name);
}

#[test]
fn test_prev_n_2() {
    let (file_name, mut reader, records) = setup_test();
    assert_eq!(reader.next_n(3).unwrap(), records[..3]);
    assert_eq!(reader.prev_n(2).unwrap(), records[1..3]);
    assert_eq!(reader.prev_n(2).unwrap(), records[0..1]);
    cleanup_test(file_name);
}
