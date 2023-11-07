use bio::io::fastq;
use bio::io::fastq::FastqRead;

#[derive(Debug, Default)]
pub struct App {
  pub quit: bool,
  pub search_patterns: Vec<(String, String)>,
  pub records_buf: Vec<fastq::Recrod>,
  file: String,
  reader: fastq::Reader,
  // buf_size
}

impl App {
  pub fn new(file: String) -> Self {
    let mut reader = fastq::Reader::from_file(file_path).expect("Failed to open fastq file");
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
          reader: reader;
          quit: false,
          search_patterns: Vec::new(),
          record_buf: records,
          file: file
      }
  }

  /// Set running to false to quit the application.
  pub fn quit(&mut self) {
    self.quit = true;
  }

  pub fn set_search_patterns(&mut self, search_patterns: Vec<(String, String)>) {
      self.search_patterns = search_patterns;
  }
}
