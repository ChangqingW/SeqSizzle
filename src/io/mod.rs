pub mod fastq;
pub mod sequence;

// Re-export for compatibility
pub use sequence::{SequenceReader, SequenceRecord, FileFormat};
