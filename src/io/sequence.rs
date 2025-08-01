use bio::io::fastq;
use flate2::read::GzDecoder;
use std::collections::{HashMap, VecDeque};
use std::env::temp_dir;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Generic sequence record that can represent both FASTQ and FASTA
#[derive(Debug, Clone, PartialEq)]
pub enum SequenceRecord {
    Fastq(fastq::Record),
    Fasta { id: String, description: Option<String>, seq: Vec<u8> },
}

impl SequenceRecord {
    pub fn id(&self) -> &str {
        match self {
            SequenceRecord::Fastq(record) => record.id(),
            SequenceRecord::Fasta { id, .. } => id,
        }
    }

    pub fn seq(&self) -> &[u8] {
        match self {
            SequenceRecord::Fastq(record) => record.seq(),
            SequenceRecord::Fasta { seq, .. } => seq,
        }
    }

    pub fn desc(&self) -> Option<&str> {
        match self {
            SequenceRecord::Fastq(record) => record.desc(),
            SequenceRecord::Fasta { description, .. } => description.as_deref(),
        }
    }

    pub fn qual(&self) -> Option<&[u8]> {
        match self {
            SequenceRecord::Fastq(record) => Some(record.qual()),
            SequenceRecord::Fasta { .. } => None,
        }
    }
}

/// File format detection
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileFormat {
    Fastq,
    Fasta,
}

impl FileFormat {
    pub fn detect_from_path(path: &Path) -> Result<Self, std::io::Error> {
        let filename = path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        
        if filename.contains(".fastq") || filename.contains(".fq") {
            Ok(FileFormat::Fastq)
        } else if filename.contains(".fasta") || filename.contains(".fna") || filename.contains(".fa") {
            Ok(FileFormat::Fasta)
        } else {
            // Try to detect from content
            Self::detect_from_content(path)
        }
    }

    fn detect_from_content(path: &Path) -> Result<Self, std::io::Error> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        
        reader.read_line(&mut line)?;
        let first_char = line.chars().next();
        
        match first_char {
            Some('@') => Ok(FileFormat::Fastq),
            Some('>') => Ok(FileFormat::Fasta),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Unable to detect file format - neither FASTQ nor FASTA"
            ))
        }
    }
}

/// Position cache for efficient backward seeking
#[derive(Debug)]
struct PositionCache {
    /// Cache file positions every N records
    cache_interval: usize,
    /// Map from record index to file position
    positions: HashMap<usize, u64>,
}

impl PositionCache {
    fn new(cache_interval: usize) -> Self {
        Self {
            cache_interval,
            positions: HashMap::new(),
        }
    }

    fn should_cache(&self, record_index: usize) -> bool {
        record_index % self.cache_interval == 0
    }

    fn insert(&mut self, record_index: usize, position: u64) {
        self.positions.insert(record_index, position);
    }

    fn find_nearest_cached_position(&self, target_index: usize) -> Option<(usize, u64)> {
        let mut best_index = None;
        let mut best_position = None;

        for (&cached_index, &position) in &self.positions {
            if cached_index <= target_index
                && (best_index.is_none() || cached_index > best_index.unwrap()) {
                    best_index = Some(cached_index);
                    best_position = Some(position);
                }
        }

        best_index.zip(best_position)
    }
}

/// Parse a FASTQ record from a BufReader
fn parse_fastq_record<R: Read>(
    buf_reader: &mut BufReader<R>,
) -> Result<Option<fastq::Record>, std::io::Error> {
    let mut id = String::new();
    let mut seq = String::new();
    let mut qual = String::new();

    let status = (
        buf_reader.read_line(&mut id),
        buf_reader.read_line(&mut seq),
        buf_reader.read_line(&mut String::new()), // skip '+'
        buf_reader.read_line(&mut qual),
    );

    match status {
        (Ok(0), Ok(0), Ok(0), Ok(0)) => Ok(None), // EOF
        (Ok(_), Ok(_), Ok(_), Ok(_)) => {
            if id.starts_with('@') {
                Ok(Some(fastq::Record::with_attrs(
                    &id.trim_end()[1..],
                    None,
                    seq.trim_end().as_bytes(),
                    qual.trim_end().as_bytes(),
                )))
            } else {
                Err(std::io::Error::other(
                    format!("ID field does not start with '@': {id}"),
                ))
            }
        }
        _ => Err(std::io::Error::other(
            "Error while parsing FASTQ lines",
        )),
    }
}

/// Parse a FASTA record from a BufReader (handles multiline sequences)
fn parse_fasta_record<R: Read + Seek>(
    buf_reader: &mut BufReader<R>,
) -> Result<Option<SequenceRecord>, std::io::Error> {
    let mut header = String::new();
    
    // Read header line
    match buf_reader.read_line(&mut header) {
        Ok(0) => return Ok(None), // EOF
        Ok(_) => {
            if !header.starts_with('>') {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("FASTA header doesn't start with '>': {header}"),
                ));
            }
        }
        Err(e) => return Err(e),
    }

    // Parse header
    let header = header.trim_end();
    let (id, description) = if let Some(space_pos) = header[1..].find(' ') {
        let id = header[1..space_pos + 1].to_string();
        let desc = Some(header[space_pos + 2..].to_string());
        (id, desc)
    } else {
        (header[1..].to_string(), None)
    };

    // Read sequence lines until next header or EOF
    let mut sequence = Vec::new();
    let mut line = String::new();
    
    loop {
        let pos_before_read = buf_reader.stream_position()?;
        line.clear();
        
        match buf_reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                if line.starts_with('>') {
                    // Next record found, seek back
                    buf_reader.seek(SeekFrom::Start(pos_before_read))?;
                    break;
                } else {
                    // Add sequence data
                    sequence.extend_from_slice(line.trim_end().as_bytes());
                }
            }
            Err(e) => return Err(e),
        }
    }

    Ok(Some(SequenceRecord::Fasta { id, description, seq: sequence }))
}

/// Skip N records in the file
fn skip_n_records<R: Read + Seek>(
    buf_reader: &mut BufReader<R>, 
    n: usize, 
    format: FileFormat
) -> Result<(), std::io::Error> {
    match format {
        FileFormat::Fastq => {
            for _ in 0..(n * 4) {
                match buf_reader.read_line(&mut String::new()) {
                    Ok(0) => return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("skip_n_records EOF reached after {n} lines"),
                    )),
                    Ok(_) => (),
                    Err(e) => return Err(e),
                }
            }
        }
        FileFormat::Fasta => {
            for _ in 0..n {
                parse_fasta_record(buf_reader)?;
            }
        }
    }
    Ok(())
}

/// Decompresses a gzipped file to a temporary file, limiting to first 10MB to avoid truncating records
fn decompress_gz_to_temp(gz_path: &Path, format: FileFormat) -> Result<(PathBuf, bool), std::io::Error> {
    let gz_file = File::open(gz_path)?;
    let mut decoder = GzDecoder::new(gz_file);
    
    let mut temp_path = temp_dir();
    temp_path.push(format!("{}.seq", Uuid::new_v4()));
    
    let mut temp_file = File::create(&temp_path)?;
    
    const MAX_DECOMPRESS_SIZE: usize = 10 * 1024 * 1024; // 10MB
    let mut buffer = vec![0u8; 64 * 1024]; // 64KB buffer
    let mut total_written = 0;
    let mut temp_data = Vec::new();
    let mut was_truncated = false;
    
    // Read and buffer data to find complete record boundaries
    loop {
        match decoder.read(&mut buffer) {
            Ok(0) => break, // EOF
            Ok(bytes_read) => {
                if total_written + bytes_read > MAX_DECOMPRESS_SIZE {
                    // We're approaching the limit - read what we can
                    let remaining = MAX_DECOMPRESS_SIZE - total_written;
                    temp_data.extend_from_slice(&buffer[..remaining.min(bytes_read)]);
                    was_truncated = true;
                    break;
                } else {
                    temp_data.extend_from_slice(&buffer[..bytes_read]);
                    total_written += bytes_read;
                }
            }
            Err(e) => return Err(e),
        }
    }
    
    // Find the last complete record boundary to avoid truncation
    if was_truncated {
        match format {
            FileFormat::Fastq => {
                // For FASTQ, find the last complete 4-line record
                let data_str = String::from_utf8_lossy(&temp_data);
                let lines: Vec<&str> = data_str.lines().collect();
                
                // Find the last position where we have a complete FASTQ record (4 lines)
                let complete_records = (lines.len() / 4) * 4;
                if complete_records > 0 {
                    // Reconstruct data up to last complete record
                    let truncated_data = lines[..complete_records].join("\n") + "\n";
                    temp_data = truncated_data.into_bytes();
                }
            }
            FileFormat::Fasta => {
                // For FASTA, find the last complete record (ends at next '>' or EOF)
                let data_str = String::from_utf8_lossy(&temp_data);
                let mut last_header_pos = 0;
                
                for (i, line) in data_str.lines().enumerate() {
                    if line.starts_with('>')
                        && i > 0 {
                            // Found a new header, previous record was complete
                            last_header_pos = data_str.lines().take(i).map(|l| l.len() + 1).sum::<usize>();
                        }
                }
                
                if last_header_pos > 0 {
                    temp_data.truncate(last_header_pos - 1); // -1 to remove trailing newline
                }
            }
        }
    }
    
    temp_file.write_all(&temp_data)?;
    temp_file.sync_all()?;
    
    if was_truncated {
        eprintln!("Warning: Gzipped file '{}' exceeds 10MB limit.", gz_path.display());
        eprintln!("Only the first ~{:.1}MB have been decompressed to avoid memory issues.", temp_data.len() as f64 / (1024.0 * 1024.0));
        eprintln!("If you need to view the entire file, please decompress it manually with: gunzip -c '{}' > uncompressed_file", gz_path.display());
    }
    
    Ok((temp_path, was_truncated))
}

// Adaptive buffer sizes based on file size
fn calculate_buffer_sizes(file_size: u64) -> (usize, usize) {
    let record_buf_size = if file_size > 100_000_000 { // > 100MB
        2048
    } else if file_size > 10_000_000 { // > 10MB
        1024
    } else {
        512
    };
    
    let reader_buf_size = record_buf_size * 4 * 1024; // 4KB per record estimate
    (record_buf_size, reader_buf_size)
}

#[derive(Debug)]
pub struct SequenceReader<R: Read + Seek> {
    buf_reader: BufReader<R>,
    records_buffer: VecDeque<SequenceRecord>,
    offset: usize, // offset of the first record in the buffer
    pub total_records: Option<usize>,
    temp_file_path: Option<PathBuf>, // Track temporary file for cleanup
    format: FileFormat,
    position_cache: PositionCache,
    record_buf_size: usize,
}

impl SequenceReader<File> {
    pub fn from_path(path: &Path) -> Result<Self, std::io::Error> {
        // Detect file format
        let format = FileFormat::detect_from_path(path)?;
        
        // Check if this is a gzipped file
        let is_gzipped = path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("gz"))
            .unwrap_or(false);
        
        if is_gzipped {
            // Decompress to temporary file and read from there (limited to 10MB)
            let (temp_path, was_truncated) = decompress_gz_to_temp(path, format)?;
            let file = File::open(&temp_path)
                .map_err(|e| std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Error opening decompressed file '{}': {}", temp_path.to_string_lossy(), e)
                ))?;
            
            let mut reader = Self::new(file, format)?;
            reader.temp_file_path = Some(temp_path);
            
            // If truncated, mark total_records as known to prevent seeking beyond
            if was_truncated {
                reader.count_all_records()?;
            }
            
            Ok(reader)
        } else {
            // Handle regular files
            let file = File::open(path)
                .map_err(|e| std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Error opening file '{}': {}", path.to_string_lossy(), e)
                ))?;
            
            if file.metadata()?.len() == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "File is empty"
                ));
            }
            
            Self::new(file, format)
        }
    }
}

impl<R: Read + Seek> SequenceReader<R> {
    pub fn new(mut reader: R, format: FileFormat) -> Result<Self, std::io::Error> {
        if reader.stream_position()? != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "reader not at the start of the file"
            ));
        }

        // Get file size for adaptive buffering
        let file_size = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;
        
        let (record_buf_size, reader_buf_size) = calculate_buffer_sizes(file_size);
        
        let mut ret = Self {
            buf_reader: BufReader::with_capacity(reader_buf_size, reader),
            records_buffer: VecDeque::with_capacity(record_buf_size + 1),
            offset: 0,
            total_records: None,
            temp_file_path: None,
            format,
            position_cache: PositionCache::new(record_buf_size / 4), // Cache every 25% of buffer size
            record_buf_size,
        };
        
        ret.fill_buffer()?;
        Ok(ret)
    }

    pub fn format(&self) -> FileFormat {
        self.format
    }

    /// Count all records in the file and cache the total
    fn count_all_records(&mut self) -> Result<(), std::io::Error> {
        let original_offset = self.offset;
        let original_buffer = self.records_buffer.clone();
        
        // Rewind to start and count all records
        self.rewind()?;
        let mut count = 0;
        
        loop {
            match self.parse_next_record()? {
                Some(_) => count += 1,
                None => break,
            }
        }
        
        self.total_records = Some(count);
        
        // Restore original position
        self.records_buffer = original_buffer;
        self.offset = original_offset;
        
        // Seek back to original position
        if original_offset > 0 {
            self.seek_to_record(original_offset)?;
        } else {
            self.rewind()?;
        }
        
        Ok(())
    }

    fn parse_next_record(&mut self) -> Result<Option<SequenceRecord>, std::io::Error> {
        match self.format {
            FileFormat::Fastq => {
                parse_fastq_record(&mut self.buf_reader)
                    .map(|opt| opt.map(SequenceRecord::Fastq))
            }
            FileFormat::Fasta => {
                parse_fasta_record(&mut self.buf_reader)
            }
        }
    }

    pub fn fill_buffer(&mut self) -> Result<(), std::io::Error> {
        for _ in 0..self.record_buf_size {
            let current_pos = self.buf_reader.stream_position()?;
            let current_record_index = self.offset + self.records_buffer.len();
            
            // Cache position if needed
            if self.position_cache.should_cache(current_record_index) {
                self.position_cache.insert(current_record_index, current_pos);
            }
            
            match self.parse_next_record()? {
                Some(record) => {
                    self.records_buffer.push_back(record);
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
        }
        Ok(())
    }

    /// Efficiently seek to a specific record index using position cache
    fn seek_to_record(&mut self, target_index: usize) -> Result<(), std::io::Error> {
        if let Some((cached_index, cached_position)) = 
            self.position_cache.find_nearest_cached_position(target_index) {
            
            // Seek to cached position
            self.buf_reader.seek(SeekFrom::Start(cached_position))?;
            self.records_buffer.clear();
            self.offset = cached_index;
            
            // Skip remaining records to reach target
            let records_to_skip = target_index.saturating_sub(cached_index);
            if records_to_skip > 0 {
                skip_n_records(&mut self.buf_reader, records_to_skip, self.format)?;
                self.offset = target_index;
            }
            
            self.fill_buffer()?;
        } else {
            // Fallback to rewind + skip
            self.rewind()?;
            if target_index > 0 {
                skip_n_records(&mut self.buf_reader, target_index, self.format)?;
                self.offset = target_index;
                self.fill_buffer()?;
            }
        }
        Ok(())
    }

    /// Get record at specific index with efficient seeking
    pub fn get_index(&mut self, index: usize) -> Result<Option<SequenceRecord>, std::io::Error> {
        if self.total_records.is_some() && index >= self.total_records.unwrap() {
            return Ok(None);
        }

        // Check if record is in current buffer
        if index >= self.offset && index < self.offset + self.records_buffer.len() {
            return Ok(Some(self.records_buffer[index - self.offset].clone()));
        }

        // For forward seeking, try to extend buffer first
        if index >= self.offset + self.records_buffer.len() {
            // Try to read forward to the target
            let records_needed = index - (self.offset + self.records_buffer.len()) + 1;
            
            for _ in 0..records_needed {
                let current_pos = self.buf_reader.stream_position()?;
                let current_record_index = self.offset + self.records_buffer.len();
                
                if self.position_cache.should_cache(current_record_index) {
                    self.position_cache.insert(current_record_index, current_pos);
                }
                
                match self.parse_next_record()? {
                    Some(record) => {
                        self.records_buffer.push_back(record);
                        if self.records_buffer.len() > self.record_buf_size {
                            self.pop_front()?;
                        }
                    }
                    None => {
                        self.total_records = Some(self.offset + self.records_buffer.len());
                        return Ok(None);
                    }
                }
            }
            
            if index >= self.offset && index < self.offset + self.records_buffer.len() {
                return Ok(Some(self.records_buffer[index - self.offset].clone()));
            }
        }

        // For backward seeking or when forward seek fails, use cached positions
        if index < self.offset {
            let target_buffer_start = index.saturating_sub(self.record_buf_size / 4);
            
            self.seek_to_record(target_buffer_start)?;
            
            if index >= self.offset && index < self.offset + self.records_buffer.len() {
                return Ok(Some(self.records_buffer[index - self.offset].clone()));
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "unable to retrieve record at index"
        ))
    }
}

impl<R: Read + Seek> Drop for SequenceReader<R> {
    fn drop(&mut self) {
        if let Some(temp_path) = &self.temp_file_path {
            if let Err(e) = std::fs::remove_file(temp_path) {
                eprintln!("Warning: Failed to remove temporary file '{}': {}", temp_path.display(), e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_test_fastq() -> (PathBuf, Vec<SequenceRecord>) {
        let mut file_name = temp_dir();
        file_name.push(format!("{}.fastq", Uuid::new_v4()));
        let mut file = File::create(file_name.clone()).unwrap();
        
        let test_data = "@id1\nAAAA\n+\nIIII\n@id2\nTTTT\n+\nIIII\n@id3\nCCCC\n+\nIIII\n";
        file.write_all(test_data.as_bytes()).unwrap();
        file.sync_all().unwrap();
        
        let expected = vec![
            SequenceRecord::Fastq(fastq::Record::with_attrs("id1", None, b"AAAA", b"IIII")),
            SequenceRecord::Fastq(fastq::Record::with_attrs("id2", None, b"TTTT", b"IIII")),
            SequenceRecord::Fastq(fastq::Record::with_attrs("id3", None, b"CCCC", b"IIII")),
        ];
        
        (file_name, expected)
    }

    fn create_test_fasta() -> (PathBuf, Vec<SequenceRecord>) {
        let mut file_name = temp_dir();
        file_name.push(format!("{}.fasta", Uuid::new_v4()));
        let mut file = File::create(file_name.clone()).unwrap();
        
        let test_data = ">id1 description 1\nAAAA\nTTTT\n>id2\nCCCC\nGGGG\n";
        file.write_all(test_data.as_bytes()).unwrap();
        file.sync_all().unwrap();
        
        let expected = vec![
            SequenceRecord::Fasta { 
                id: "id1".to_string(), 
                description: Some("description 1".to_string()), 
                seq: b"AAAATTTT".to_vec() 
            },
            SequenceRecord::Fasta { 
                id: "id2".to_string(), 
                description: None, 
                seq: b"CCCCGGGG".to_vec() 
            },
        ];
        
        (file_name, expected)
    }

    #[test]
    fn test_fastq_reading() {
        let (file_path, expected) = create_test_fastq();
        let mut reader = SequenceReader::new(File::open(&file_path).unwrap(), FileFormat::Fastq).unwrap();
        
        assert_eq!(reader.format(), FileFormat::Fastq);
        assert_eq!(reader.get_index(0).unwrap().unwrap(), expected[0]);
        assert_eq!(reader.get_index(1).unwrap().unwrap(), expected[1]);
        assert_eq!(reader.get_index(2).unwrap().unwrap(), expected[2]);
        
        std::fs::remove_file(file_path).unwrap();
    }

    #[test]
    fn test_fasta_reading() {
        let (file_path, expected) = create_test_fasta();
        let mut reader = SequenceReader::new(File::open(&file_path).unwrap(), FileFormat::Fasta).unwrap();
        
        assert_eq!(reader.format(), FileFormat::Fasta);
        assert_eq!(reader.get_index(0).unwrap().unwrap(), expected[0]);
        assert_eq!(reader.get_index(1).unwrap().unwrap(), expected[1]);
        
        std::fs::remove_file(file_path).unwrap();
    }

    #[test]
    fn test_position_cache() {
        let (file_path, _) = create_test_fastq();
        let mut reader = SequenceReader::new(File::open(&file_path).unwrap(), FileFormat::Fastq).unwrap();
        
        // Read forward to populate cache
        reader.get_index(2).unwrap();
        
        // Read backward - should use cache
        reader.get_index(0).unwrap();
        
        std::fs::remove_file(file_path).unwrap();
    }
}
