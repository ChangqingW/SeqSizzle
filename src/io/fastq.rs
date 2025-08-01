use bio::io::fastq;
use flate2::read::GzDecoder;
use std::collections::VecDeque;
use std::env::temp_dir;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

// WIP: refactor
// use a VecDeque to buffer records
// store index offset of the vecdeque

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

    #[allow(clippy::type_complexity)]
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
                    seq.trim_end().as_bytes(),
                    qual.trim_end().as_bytes(),
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
#[allow(dead_code)]
fn try_next<R: Read + Seek>(
    buf_reader: &mut BufReader<R>,
) -> Result<fastq::Record, std::io::Error> {
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
/// return None if EOF is reached
fn next<R: Read + Seek>(
    buf_reader: &mut BufReader<R>,
) -> Result<Option<fastq::Record>, std::io::Error> {
    let rec: Option<fastq::Record> = parse_record(buf_reader)?;
    if let Some(rec) = rec {
        Ok(Some(rec))
    } else {
        Ok(None)
    }
}

/// Read records from a BufReader until the given position is reached
#[allow(dead_code)]
fn read_to_pos<R: Read + Seek>(
    buf_reader: &mut BufReader<R>,
    pos: u64,
) -> Result<VecDeque<fastq::Record>, std::io::Error> {
    let mut buff: VecDeque<fastq::Record> = VecDeque::with_capacity(RECORD_BUF_SIZE + 1);
    buff.push_back(try_next(buf_reader)?);
    loop {
        let res: Option<fastq::Record> = next(buf_reader)?;
        match res {
            Some(rec) => {
                let current_pos = buf_reader.stream_position()?;
                match current_pos.cmp(&pos) {
                    std::cmp::Ordering::Equal => {
                        buff.push_back(rec);
                        break;
                    }
                    std::cmp::Ordering::Greater => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "read_to_pos exceeded given pos",
                        ));
                    }
                    std::cmp::Ordering::Less => {
                        buff.push_back(rec);
                        if buff.len() >= RECORD_BUF_SIZE {
                            buff.pop_front();
                        }
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

fn skip_n_records<R: Read>(buf_reader: &mut BufReader<R>, n: usize) -> Result<(), std::io::Error> {
    for _ in 0..(n * 4) {
        match buf_reader.read_line(&mut String::new()) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("skip_n_records EOF reached after {} lines", n),
                ));
            }
            Ok(_) => (),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(not(debug_assertions))]
static RECORD_BUF_SIZE: usize = 1024;
#[cfg(not(debug_assertions))]
static READER_BUF_SIZE: usize = RECORD_BUF_SIZE * 4 * 1024; // 4MB

#[cfg(debug_assertions)]
static RECORD_BUF_SIZE: usize = 4;
#[cfg(debug_assertions)]
static READER_BUF_SIZE: usize = RECORD_BUF_SIZE * 4 * 300;


/// Decompresses a gzipped file to a temporary file and returns the path
fn decompress_gz_to_temp(gz_path: &Path) -> Result<PathBuf, std::io::Error> {
    let gz_file = File::open(gz_path)?;
    let mut decoder = GzDecoder::new(gz_file);
    
    let mut temp_path = temp_dir();
    temp_path.push(format!("{}.fastq", Uuid::new_v4()));
    
    let mut temp_file = File::create(&temp_path)?;
    std::io::copy(&mut decoder, &mut temp_file)?;
    temp_file.sync_all()?;
    
    Ok(temp_path)
}

#[derive(Debug)]
pub struct FastqReader<R: Read + Seek> {
    buf_reader: BufReader<R>,
    records_buffer: VecDeque<fastq::Record>,
    offset: usize, // offset of the first record in the buffer
    pub total_records: Option<usize>,
    temp_file_path: Option<PathBuf>, // Track temporary file for cleanup
}

// Constructor for File
impl FastqReader<File> {
    pub fn from_path(path: &Path) -> Result<Self, std::io::Error> {
        // Check if this is a gzipped file
        let is_gzipped = path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("gz"))
            .unwrap_or(false);
        
        if is_gzipped {
            // Decompress to temporary file and read from there
            let temp_path = decompress_gz_to_temp(path)?;
            let file = File::open(&temp_path)
                .map_err(|e| std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Error opening decompressed file '{}': {}", temp_path.to_string_lossy(), e)
                ))?;
            
            let mut reader = Self::new(file)?;
            reader.temp_file_path = Some(temp_path);
            Ok(reader)
        } else {
            // Handle regular files
            let mut file = File::open(path)
                .map_err(|e| std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Error opening file '{}': {}", path.to_string_lossy(), e)
                ))?;
            
            if file.stream_position().is_err() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "File not seekable, are you using a pipe? Consider saving to an actual file"
                ));
            }
            
            Self::new(file)
        }
    }
}

// Generic methods
impl<R: Read + Seek> FastqReader<R> {
    pub fn new(mut reader: R) -> Result<Self, std::io::Error> {
        if reader.stream_position()? != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "reader not at the start of the file"
            ));
        }
        let mut ret = Self {
            buf_reader: BufReader::with_capacity(READER_BUF_SIZE, reader),
            records_buffer: VecDeque::with_capacity(RECORD_BUF_SIZE + 1),
            offset: 0,
            total_records: None,
            temp_file_path: None,
        };
        ret.fill_buffer()?;
        Ok(ret)
    }

    pub fn fill_buffer(&mut self) -> Result<(), std::io::Error> {
        for _ in 0..RECORD_BUF_SIZE {
            match next(&mut self.buf_reader)? {
                Some(res) => {
                    self.records_buffer.push_back(res);
                }
                None => {
                    self.total_records = Some(self.offset + self.records_buffer.len());
                    break;
                }
            }
        }
        Ok(())
    }

    fn pop_front(&mut self) -> Result<(), std::io::Error> {
        if self.records_buffer.pop_front().is_some() {
            self.offset += 1;
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "pop_front called on empty buffer"
            ))
        }
    }

    pub fn rewind(&mut self) -> Result<(), std::io::Error> {
        if self.offset != 0 {
            self.records_buffer.clear();
            self.buf_reader.rewind()?;
            self.offset = 0;
            self.fill_buffer()?;
            Ok(())
        } else {
            Ok(())
        }
    }

    /// returns the record at the given index
    /// if the index is after the current buffer, forward the buffer to RECORD_BUF_SIZE/4
    /// records after the index
    /// if the index is before the current buffer, rewind the buffer to RECORD_BUF_SIZE/4
    /// records before the index
    pub fn get_index(&mut self, index: usize) -> Result<Option<fastq::Record>, std::io::Error> {
        if self.total_records.is_some() && index > self.total_records.unwrap() {
            return Ok(None);
        }
        if index >= self.offset && index < self.offset + self.records_buffer.len() {
            Ok(Some(self.records_buffer[index - self.offset].clone()))
        } else if index >= self.offset + self.records_buffer.len() {
            // forward the buffer
            for _ in 0..(index - self.offset - self.records_buffer.len() + RECORD_BUF_SIZE / 4) {
                match next(&mut self.buf_reader)? {
                    Some(res) => {
                        self.records_buffer.push_back(res);
                        if self.records_buffer.len() > RECORD_BUF_SIZE {
                            self.pop_front()?;
                        }
                    }
                    None => {
                        self.total_records = Some(self.offset + self.records_buffer.len());
                        if index >= self.offset + self.records_buffer.len() {
                            return Ok(None);
                        }
                        break;
                    }
                }
            }

            return Ok(Some(self.records_buffer[index - self.offset].clone()));
        } else if index < self.offset {
            // rewind the buffer
            if index < RECORD_BUF_SIZE {
                self.rewind()?;
                return Ok(Some(self.records_buffer[index].clone()));
            } else {
                self.records_buffer.clear();
                self.buf_reader.rewind()?;
                // TODO: seek backwards instead of rewinding
                skip_n_records(&mut self.buf_reader, index - RECORD_BUF_SIZE / 4)?;
                self.offset = index - RECORD_BUF_SIZE / 4;
                self.fill_buffer()?;
                return Ok(Some(self.records_buffer[RECORD_BUF_SIZE / 4].clone()));
            }
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unexpected case in get_index"
            ))
        }
    }
}

impl<R: Read + Seek> Drop for FastqReader<R> {
    fn drop(&mut self) {
        if let Some(temp_path) = &self.temp_file_path {
            if let Err(e) = std::fs::remove_file(temp_path) {
                eprintln!("Warning: Failed to remove temporary file '{}': {}", temp_path.display(), e);
            }
        }
    }
}

#[allow(dead_code)]
fn setup_test() -> (PathBuf, FastqReader<File>, Vec<fastq::Record>) {
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
    let reader = FastqReader::new(File::open(file_name.clone()).unwrap()).unwrap();
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
fn test_get_index() {
    let (file_name, mut reader, records) = setup_test();
    assert_eq!(reader.get_index(0).unwrap().unwrap(), records[0]);
    assert_eq!(reader.get_index(9).unwrap().unwrap(), records[9]);
    assert_eq!(reader.get_index(4).unwrap().unwrap(), records[4]);
    assert_eq!(reader.get_index(8).unwrap().unwrap(), records[8]);
    assert_eq!(reader.get_index(5).unwrap().unwrap(), records[5]);
    cleanup_test(file_name);
}
