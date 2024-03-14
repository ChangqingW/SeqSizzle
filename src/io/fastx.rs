use crate::io::fastq::FastqReader;
use bio::io::fastq;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

pub trait FastxReader<R: Read + Seek> : std::fmt::Debug {
    fn fill_buffer(&mut self) -> Result<(), std::io::Error>;
    fn rewind(&mut self) -> Result<(), std::io::Error>;
    fn get_index(&mut self, index: usize) -> Result<Option<fastq::Record>, std::io::Error>;
}

pub fn from_path(path: &Path) -> impl FastxReader<File> {
    // if .fastq
    FastqReader::new(match File::open(path) {
        Ok(mut file) => {
            assert!(
                file.stream_position().is_ok(),
                "File not seekable, are you using a pipe? Consider saving to an actual file"
            );
            file
        }
        Err(e) => panic!("Error opening file '{}': {:?}", path.to_string_lossy(), e),
    })
    // else if .fasta
    // else
}
