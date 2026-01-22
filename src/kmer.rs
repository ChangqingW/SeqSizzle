use crate::io::{SequenceReader, SequenceRecord};
use anyhow::Result;
use clap::Args;
use rayon::prelude::*;
use rand::Rng;
use rand::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::ops::Range;

/// Statistics for a k-mer including enrichment metrics
#[derive(Debug, Clone)]
pub struct KmerStats {
    pub sequence: Vec<u8>,
    pub observed_count: u64,
    pub expected_count: f64,
    pub sqrt_deviance: f64,
    pub log_fold_enrichment: f64,
    pub is_homopolymer: bool,
}

impl KmerStats {
    /// Create new k-mer statistics
    fn new(
        sequence: Vec<u8>, 
        observed_count: u64, 
        expected_count: f64,
        // total_sequences: usize, // for multiple testing correction
    ) -> Self {
        
        // Calculate square root deviance (0 if observed <= expected)
        let sqrt_deviance = if observed_count as f64 > expected_count {
            let deviance = 2.0 * (observed_count as f64 * (observed_count as f64 / expected_count).ln() - 
                                 (observed_count as f64 - expected_count));
            deviance.sqrt()
        } else {
            0.0
        };
        
        // Calculate log fold enrichment
        let log_fold_enrichment = if expected_count > 0.0 {
            (observed_count as f64 / expected_count).log2()
        } else {
            f64::INFINITY
        };
        
        // Calculate homopolymer status once
        let is_homopolymer = is_homopolymer(&sequence);
        
        Self {
            sequence,
            observed_count,
            expected_count,
            sqrt_deviance,
            log_fold_enrichment,
            is_homopolymer,
        }
    }
    
    /// Calculate expected count for a sequence of given length
    fn calculate_expected_count(sequence_length: usize, total_length: usize) -> f64 {
        let total_possible = total_length.saturating_sub(sequence_length - 1);
        total_possible as f64 / (4_f64.powi(sequence_length as i32))
    }
    
    /// Create stats from observed count and total length
    fn from_count(sequence: Vec<u8>, observed_count: u64, total_length: usize) -> Self {
        let expected = Self::calculate_expected_count(sequence.len(), total_length);
        Self::new(sequence, observed_count, expected)
    }
}

/// Source of an enriched sequence
#[derive(Debug, Clone)]
pub enum SequenceSource {
    /// K-mer from a specific k-value
    Kmer(usize),
    /// Assembled sequence from k-mers of a specific k-value
    Assembled { from_k: usize },
}

impl SequenceSource {
    /// Format the source for CSV output
    fn format_for_csv(&self) -> String {
        match self {
            SequenceSource::Kmer(k) => k.to_string(),
            SequenceSource::Assembled { from_k } => format!("assembled from k={from_k}"),
        }
    }
}

/// Represents an enriched sequence with all its metadata for output
#[derive(Debug, Clone)]
pub struct EnrichedSequence {
    pub sequence: String,
    pub length: usize,
    pub count_display: String,
    pub counts_per_read: f64,
    pub source: SequenceSource,
    pub sqrt_deviance: f64,
    pub log_fold_enrichment: f64,
}

impl EnrichedSequence {
    /// Write this sequence to a CSV writer
    fn write_to_csv<W: std::io::Write>(&self, writer: &mut csv::Writer<W>) -> Result<()> {
        let formatted_log_fold = if self.log_fold_enrichment.is_infinite() {
            if self.log_fold_enrichment.is_sign_positive() {
                "inf".to_string()
            } else {
                "-inf".to_string()
            }
        } else {
            format!("{:.1}", self.log_fold_enrichment)
        };
        
        writer.write_record([
            &self.sequence,
            &self.length.to_string(),
            &self.count_display,
            &format!("{:.2}", self.counts_per_read),
            &self.source.format_for_csv(),
            &format!("{:.1}", self.sqrt_deviance),
            &formatted_log_fold,
        ])?;
        Ok(())
    }
}

/// Result of merging sequences with their reverse complements
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub sequence: Vec<u8>,
    pub total_count: u64,
    pub forward_count: Option<u64>,  // Some if merged, None if single
    pub reverse_count: Option<u64>,  // Some if merged, None if single
    pub stats: KmerStats,            // Statistical information
}

impl MergeResult {
    /// Create a result for a single sequence (not merged)
    fn single(sequence: Vec<u8>, count: u64, stats: KmerStats) -> Self {
        Self {
            sequence,
            total_count: count,
            forward_count: None,
            reverse_count: None,
            stats,
        }
    }
    
    /// Create a result for merged sequences
    fn merged(sequence: Vec<u8>, forward_count: u64, reverse_count: u64, stats: KmerStats) -> Self {
        Self {
            sequence,
            total_count: forward_count + reverse_count,
            forward_count: Some(forward_count),
            reverse_count: Some(reverse_count),
            stats,
        }
    }
    
    /// Format the count for display
    pub fn format_count(&self) -> String {
        match (self.forward_count, self.reverse_count) {
            (Some(forward), Some(reverse)) => {
                format!("{} (+{}-{})", self.total_count, forward, reverse)
            }
            _ => self.total_count.to_string(),
        }
    }
    
    /// Convert to EnrichedSequence for output
    pub fn to_enriched_sequence(&self, read_count: usize, source: SequenceSource) -> EnrichedSequence {
        EnrichedSequence {
            sequence: String::from_utf8_lossy(&self.sequence).into_owned(),
            length: self.sequence.len(),
            count_display: self.format_count(),
            counts_per_read: self.total_count as f64 / read_count as f64,
            source,
            sqrt_deviance: self.stats.sqrt_deviance,
            log_fold_enrichment: self.stats.log_fold_enrichment,
        }
    }
}

#[derive(Debug, Args, Clone)]
pub struct KmerEnrichmentArgs {
    /// Path to write the output CSV file.
    #[clap(short, long)]
    pub output: PathBuf,

    /// Limit the total number of reads used for enrichment.
    /// Set to 0 to use all reads.
    #[clap(long, default_value_t = 10000)]
    pub max_reads: usize,

    /// If set, randomly sample `--max-reads` reads from the file instead of taking the first N.
    /// Requires `--max-reads`.
    #[clap(long)]
    pub sample: bool,

    /// Minimum k-mer length to check.
    #[clap(long, default_value_t = 8)]
    pub k_min: usize,

    /// Maximum k-mer length to check.
    #[clap(long, default_value_t = 12)]
    pub k_max: usize,

    /// Step size between k-values (arithmetic progression).
    #[clap(long, default_value_t = 2)]
    pub k_step: usize,

    /// Number of top k-mers to keep per k value.
    #[clap(long, default_value_t = 400)]
    pub top_kmers: usize,

    /// Substring filtering counts ratio threshold.
    /// For k-mers that are contained within longer k-mers, those with
    /// (shorter k-mer count) / (longer k-mer count) >= this threshold will be removed.
    /// For homopolymer k-mers, the threshold is lowered to threshold^4.
    #[clap(long, default_value_t = 0.8)]
    pub substring_count_ratio_threshold: f64,

    /// Minimum counts per read threshold for k-mers (overrides z-score if provided).
    /// Accepts fractional values (e.g., 0.01 for 1 count per 100 reads).
    #[clap(long)]
    pub min_count: Option<f64>,

    /// Z-score threshold for k-mer enrichment (default: 5.0).
    #[clap(long, default_value_t = 5.0)]
    pub z_score_threshold: f64,

    /// Perform assembly with k_max k-mers
    #[clap(long, default_value_t = false)]
    pub skip_assemble: bool,

    /// Detect and merge reverse complement k-mers.
    #[clap(long)]
    pub detect_reverse_complement: bool,

    /// Anti-reference FASTA/FASTQ file. All k-mers present in this file (for the same k values)
    /// are used as a blacklist and will be excluded from enrichment results.
    #[clap(long = "anti-reference")]
    pub anti_reference: Option<PathBuf>,

    /// Anti-reference error rate (0.0-1.0). If set and `--anti-reference` is provided,
    /// additionally blacklist all k-mers within floor(k * error_rate) substitutions of each
    /// blacklisted k-mer.
    #[clap(long = "anti-reference-error-rate", default_value_t = 0.0)]
    pub anti_reference_error_rate: f64,
}

/// Configuration struct for centralized parameter management
#[derive(Debug, Clone)]
pub struct KmerConfig {
    pub k_range: Range<usize>,
    pub k_step: usize,
    pub top_kmers: usize,
    pub substring_count_ratio_threshold: f64,
    pub z_score_threshold: f64,
    pub min_count: Option<f64>,
    pub output_path: PathBuf,
    pub assemble: bool,
    pub detect_reverse_complement: bool,
    pub anti_reference_path: Option<PathBuf>,
    pub anti_reference_error_rate: f64,
}

impl KmerConfig {
    /// Create a new configuration from CLI arguments
    pub fn from_args(args: &KmerEnrichmentArgs) -> Result<Self> {
        
        validate_args(args)?;
        
        let output_path = if args.output.is_absolute() {
            args.output.clone()
        } else {
            std::env::current_dir()?.join(&args.output)
        };
        
        Ok(KmerConfig {
            k_range: args.k_min..args.k_max + 1,
            k_step: if args.k_max == args.k_min { 0 } else { args.k_step },
            top_kmers: args.top_kmers,
            substring_count_ratio_threshold: args.substring_count_ratio_threshold,
            z_score_threshold: args.z_score_threshold,
            min_count: args.min_count,
            output_path,
            assemble: !args.skip_assemble,
            detect_reverse_complement: args.detect_reverse_complement,
            anti_reference_path: args.anti_reference.clone(),
            anti_reference_error_rate: args.anti_reference_error_rate,
        })
    }
    
    /// Get the k-values to process as an arithmetic sequence
    pub fn k_values(&self) -> Vec<usize> {
        if self.k_step == 0 {
            vec![self.k_range.start]
        } else {
            (self.k_range.start..self.k_range.end)
                .step_by(self.k_step)
                .collect()
        }
    }
    
    /// Get filtering method description for logging
    pub fn filter_method_description(&self) -> String {
        match self.min_count {
            Some(count) => format!("min_count={count}"),
            None => format!("z_score_threshold={}", self.z_score_threshold),
        }
    }
}

/// Validate CLI arguments and return errors for invalid combinations
fn validate_args(args: &KmerEnrichmentArgs) -> Result<()> {
    if args.sample && args.max_reads == 0 {
        return Err(anyhow::anyhow!("--sample requires --max-reads to be set greater than 0"));
    }
    
    if args.k_min > args.k_max {
        return Err(anyhow::anyhow!(
            "k-min ({}) must be less than or equal to k-max ({})",
            args.k_min, args.k_max
        ));
    }
    
    if !(args.k_min == args.k_max) && args.k_step == 0 {
        return Err(anyhow::anyhow!("k-step must be greater than 0"));
    }
    
    if !(args.k_min == args.k_max) && args.k_step > (args.k_max - args.k_min) {
        return Err(anyhow::anyhow!(
            "k-step ({}) is too large for the given k-range ({} to {})",
            args.k_step, args.k_min, args.k_max
        ));
    }
    
    if args.z_score_threshold < 0.1 || args.z_score_threshold > 20.0 {
        return Err(anyhow::anyhow!(
            "z-score-threshold ({}) must be between 0.1 and 20.0",
            args.z_score_threshold
        ));
    }
    
    if args.top_kmers == 0 {
        return Err(anyhow::anyhow!("top-kmers must be greater than 0"));
    }
    
    // Check if output directory exists and is writable
    let output_path = if args.output.is_absolute() {
        args.output.clone()
    } else {
        std::env::current_dir()?.join(&args.output)
    };
    
    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            return Err(anyhow::anyhow!(
                "Output directory does not exist: {}",
                parent.display()
            ));
        }
    }
    
    // Validate anti-reference path if provided
    if let Some(ref anti_ref) = args.anti_reference {
        if !anti_ref.exists() {
            return Err(anyhow::anyhow!(
                "Anti-reference file does not exist: {}",
                anti_ref.display()
            ));
        }
    }

    if args.anti_reference_error_rate < 0.0 || args.anti_reference_error_rate >= 1.0 {
        return Err(anyhow::anyhow!(
            "anti-reference-error-rate ({}) must be between 0.0 and 1.0",
            args.anti_reference_error_rate
        ));
    }

    Ok(())
}


// helpfer functions
//
/// Compute the reverse complement of a DNA sequence
fn reverse_complement(seq: &[u8]) -> Vec<u8> {
    seq.iter()
        .rev()
        .map(|&base| match base.to_ascii_uppercase() {
            b'A' => b'T',
            b'T' => b'A',
            b'G' => b'C',
            b'C' => b'G',
            _ => base, // Keep ambiguous bases as-is
        })
        .collect()
}

// TODO: make threshold configurable, revise definition
/// Check if a k-mer is a homopolymer (>80% same base)
fn is_homopolymer(kmer: &[u8]) -> bool {
    if kmer.is_empty() {
        return false;
    }
    
    let mut counts = [0; 4]; // A, T, G, C
    for &base in kmer {
        match base.to_ascii_uppercase() {
            b'A' => counts[0] += 1,
            b'T' => counts[1] += 1,
            b'G' => counts[2] += 1,
            b'C' => counts[3] += 1,
            _ => {} // Skip ambiguous bases
        }
    }
    
    let max_count = counts.iter().max().unwrap_or(&0);
    let threshold = (kmer.len() as f64 * 0.8) as usize;
    *max_count >= threshold
}

// TODO: make generic function with threshold?
/// Check if a k-mer is a pure homopolymer (100% same base)
fn is_pure_homopolymer(kmer: &[u8]) -> bool {
    if kmer.is_empty() {
        return false;
    }
    let first = kmer[0];
    kmer.iter().all(|&b| b == first)
}

// K-mer processing
// 
fn count_kmers_with_min_count(
    records: &[SequenceRecord], 
    k: usize, 
    min_threshold: u64,
    total_length: usize
) -> HashMap<Vec<u8>, KmerStats> {
    // Calculate expected frequency for statistical calculations
    let total_possible_kmers = total_length.saturating_sub(k - 1);
    let expected_frequency = total_possible_kmers as f64 / (4_f64.powi(k as i32));

    // Count k-mers
    let kmer_counts: HashMap<Vec<u8>, u64> = records
        .par_iter()
        .flat_map(|record| record.seq().par_windows(k))
        .fold(HashMap::new, |mut acc, kmer| {
            *acc.entry(kmer.to_vec()).or_insert(0) += 1;
            acc
        })
        .reduce(HashMap::new, |mut a, b| {
            for (kmer, count) in b {
                *a.entry(kmer).or_insert(0) += count;
            }
            a
        });

    // Filter and convert to KmerStats
    kmer_counts
        .into_iter()
        .filter(|(_, count)| *count >= min_threshold)
        .map(|(kmer, count)| {
            let stats = KmerStats::new(kmer.clone(), count, expected_frequency);
            (kmer, stats)
        })
        .collect()
}

fn count_kmers_with_zscore_stats(
    records: &[SequenceRecord], 
    k: usize, 
    total_length: usize,
    z_threshold: f64
) -> HashMap<Vec<u8>, KmerStats> {
    // Calculate expected frequency for random k-mers
    // For equal probability of 4 bases: P(specific k-mer) = (1/4)^k
    // Expected count = total_possible_kmers * P(specific k-mer)
    let total_possible_kmers = total_length.saturating_sub(k - 1);
    let expected_frequency = total_possible_kmers as f64 / (4_f64.powi(k as i32));

    // For Binomial distribution, std = sqrt(n * p * (1 - p))
    let expected_std = (expected_frequency * (1.0 - (1.0 / 4_f64.powi(k as i32)))).sqrt();
    let min_count_threshold = (expected_frequency + z_threshold * expected_std).ceil().max(2.0) as u64;

    println!("  Expected frequency: {expected_frequency:.2}, std: {expected_std:.2}, min_count (Z≥{z_threshold:.1}): {min_count_threshold}");

    // Count k-mers
    let kmer_counts: HashMap<Vec<u8>, u64> = records
        .par_iter()
        .flat_map(|record| record.seq().par_windows(k))
        .fold(HashMap::new, |mut acc, kmer| {
            *acc.entry(kmer.to_vec()).or_insert(0) += 1;
            acc
        })
        .reduce(HashMap::new, |mut a, b| {
            for (kmer, count) in b {
                *a.entry(kmer).or_insert(0) += count;
            }
            a
        });

    // Filter and convert to KmerStats
    kmer_counts
        .into_iter()
        .filter(|(_, count)| *count >= min_count_threshold)
        .map(|(kmer, count)| {
            let stats = KmerStats::new(kmer.clone(), count, expected_frequency);
            (kmer, stats)
        })
        .collect()
}


/// Load all records from an anti-reference FASTA/FASTQ file.
fn load_anti_reference_records(anti_reference_path: &Path) -> Result<Vec<SequenceRecord>> {
    let mut reader = SequenceReader::from_path(anti_reference_path)?;
    let mut records = Vec::new();
    let mut index = 0;
    while let Some(record) = reader.get_index(index)? {
        records.push(record);
        index += 1;
    }
    Ok(records)
}

/// Returns true if `pattern` matches any substring of `text` within edit distance `dist`.
/// Uses the same `bio::pattern_matching::myers` approach as `search_generic` in `app.rs`.
fn matches_any_within_edit_distance(pattern: &[u8], text: &[u8], dist: usize, k: usize) -> bool {
    use bio::pattern_matching::myers::{Myers, MyersBuilder};

    if pattern.is_empty() {
        return false;
    }

    // Exact match fast-path
    if dist == 0 {
        return text
            .windows(pattern.len())
            .any(|w| w.eq_ignore_ascii_case(pattern));
    }

    let dist: u8 = dist.try_into().expect("Edit distance too large for Myers algorithm (u8)");

    let mut builder = MyersBuilder::new();
    for (base, equivalents) in vec![
        (b'M', &b"AC"[..]),
        (b'R', &b"AG"[..]),
        (b'W', &b"AT"[..]),
        (b'S', &b"CG"[..]),
        (b'Y', &b"CT"[..]),
        (b'K', &b"GT"[..]),
        (b'V', &b"ACGMRS"[..]),
        (b'H', &b"ACTMWY"[..]),
        (b'D', &b"AGTRWK"[..]),
        (b'B', &b"CGTSYK"[..]),
        (b'N', &b"ACGTMRWSYKVHDB"[..]),
    ] {
        builder.ambig(base, equivalents);
    }

    if k < 8 {
        let mut myers: Myers<u8> = builder.build(pattern);
        myers
            .find_all(text, dist)
            .next()
            .is_some()
    } else if k < 16 {
        let mut myers: Myers<u16> = builder.build(pattern);
        myers
            .find_all(text, dist)
            .next()
            .is_some()
    } else if k < 32 {
        let mut myers: Myers<u32> = builder.build(pattern);
        myers
            .find_all(text, dist)
            .next()
            .is_some()
    } else if k < 64 {
        let mut myers: Myers<u64> = builder.build(pattern);
        myers
            .find_all(text, dist)
            .next()
            .is_some()
    } else {
        panic!("K-mer length too large for Myers algorithm");
    }
}

/// Select top k-mers with balanced approach for homopolymers
fn select_top_kmers(
    kmer_stats: HashMap<Vec<u8>, KmerStats>, 
    k: usize, 
    top_n: usize,
    anti_reference_records: Option<&[SequenceRecord]>,
    anti_reference_error_rate: f64,
) -> HashMap<Vec<u8>, KmerStats> {
    let initial_count = kmer_stats.len();
    
    if initial_count <= top_n {
        println!("  {initial_count} {k}-mers selected (keeping all)");
        return kmer_stats;
    }

    // Separate homopolymers from other sequences
    let (homopolymers, other_kmers): (Vec<_>, Vec<_>) = kmer_stats
        .iter()
        .partition(|(_, stats)| stats.is_homopolymer);

    // TODO: simplify this with pure / nosiy homopolymer filtering?
    // Strategy: Keep some homopolymers, but focus on non-homopolymer sequences
    let max_homopolymers = (top_n / 10).clamp(4, 20); // 10% for homopolymers, 4-20 range
    let mut remaining_slots = top_n.saturating_sub(max_homopolymers.min(homopolymers.len()));

    let mut result = HashMap::new();

    // Add top non-homopolymer sequences (optionally anti-reference-filtered)
    let mut sorted_others: Vec<_> = other_kmers.into_iter().collect();
    sorted_others.sort_by(|a, b| b.1.observed_count.cmp(&a.1.observed_count));

    let max_errors = ((k as f64) * anti_reference_error_rate).ceil() as usize;

    for (kmer, stats) in sorted_others {
        if remaining_slots <= 0 {
            break;
        }

        // If anti-reference provided, discard k-mers that match any anti-reference record
        // within the allowed edit distance.
        if let Some(anti_records) = anti_reference_records {
            let mut hit = false;
            for rec in anti_records {
                if matches_any_within_edit_distance(&kmer, rec.seq(), max_errors, k) {
                    hit = true;
                    break;
                }
            }
            if hit {
                continue;
            }
        }

        result.insert(kmer.clone(), stats.clone());
        remaining_slots = remaining_slots.saturating_sub(1);
    }
    
    let homo_count = result.iter().filter(|(_, stats)| stats.is_homopolymer).count();
    let other_count = result.len() - homo_count;
    println!("  {} {k}-mers selected ({homo_count} homopolymers, {other_count} others)", 
        result.len());
    
    result
}

// filtering & assembly
//
fn is_substring(sub: &[u8], main: &[u8]) -> bool {
    main.windows(sub.len()).any(|window| window == sub)
}

fn filter_substrings(
    mut enriched_kmers: HashMap<usize, HashMap<Vec<u8>, KmerStats>>,
    threshold: f64,
) -> HashMap<usize, HashMap<Vec<u8>, KmerStats>> {
    // Get all k-values that are actually present (in case of step > 1)
    let mut k_values: Vec<usize> = enriched_kmers.keys().copied().collect();
    k_values.sort();
    let k_max = *k_values.last().unwrap();
    
    // Process in increasing k order: k_min to k_max-1
    // This ensures shorter k-mers are filtered out by longer ones iteratively
    for &k in &k_values {
        if k >= k_max {
            continue; // Don't filter the largest k-value
        }
        
        let short_kmers = if let Some(short_kmers) = enriched_kmers.get(&k) {
            short_kmers.clone()
        } else {
            continue;
        };

        let original_len = short_kmers.len();
        
        let surviving_kmers: HashMap<Vec<u8>, KmerStats> = short_kmers
            .into_par_iter()
            .filter(|(kmer_short, stats_short)| {
                // Check against ALL longer k-mers in our k_values
                let should_remove = k_values.iter()
                    .filter(|&&long_k| long_k > k)
                    .any(|&long_k| {
                        if let Some(long_kmers) = enriched_kmers.get(&long_k) {
                            long_kmers.iter().any(|(kmer_long, stats_long)| {
                                let ratio = stats_long.observed_count as f64 / stats_short.observed_count as f64;
                                let is_sub = is_substring(kmer_short, kmer_long);
                                // if both are homopolymers, use threshold^4
                                if stats_short.is_homopolymer && stats_long.is_homopolymer {
                                    ratio >= threshold.powi(4) && is_sub
                                } else {
                                    ratio >= threshold && is_sub
                                }
                            })
                        } else {
                            false
                        }
                    });
                
                !should_remove
            })
            .collect();

        let removed_count = original_len - surviving_kmers.len();
        if removed_count > 0 {
            println!("  k={k}: {original_len} -> {} after substring filtering ({removed_count} removed)",
                surviving_kmers.len());
        }
        enriched_kmers.insert(k, surviving_kmers);
    }
    enriched_kmers
}

/// Assemble k-mers into longer sequences by finding linear paths in an overlap graph.
/// 
/// This function is designed for identifying primer/adapter sequences where:
/// - Branching indicates distinct sequences that should be kept separate
/// - Assembly stops at any ambiguity (multiple outgoing edges)
/// - Minimum count is used as a conservative estimate of sequence abundance
/// 
/// Returns a HashMap of assembled sequences with their minimum k-mer counts.
fn assemble_kmers(
    enriched_kmers: &HashMap<Vec<u8>, KmerStats>,
    k: usize,
    edge_join_threshold: f64,
) -> HashMap<Vec<u8>, u64> {
    if enriched_kmers.is_empty() {
        return HashMap::new();
    }

    // Filter out all homopolymer k-mers before assembly
    let non_homopolymer_kmers: HashMap<Vec<u8>, KmerStats> = enriched_kmers
        .iter()
        .filter(|(_, stats)| !stats.is_homopolymer)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    
    if non_homopolymer_kmers.is_empty() {
        println!("  All k-mers are homopolymers, skipping assembly.");
        return HashMap::new();
    }
    
    let filtered_count = enriched_kmers.len() - non_homopolymer_kmers.len();
    if filtered_count > 0 {
        println!("  Filtered out {filtered_count} homopolymer k-mers from assembly");
    }

    // Build overlap graph with stricter overlap criteria
    let mut graph: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
    let mut reverse_graph: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();

    let kmers: Vec<_> = non_homopolymer_kmers.keys().collect();
    
    // Debug: print k-mers being processed in tests
    #[cfg(test)]
    {
        println!("Building graph for {} k-mers (k={})", kmers.len(), k);
        for kmer in &kmers {
            let stats = non_homopolymer_kmers.get(*kmer).unwrap();
            println!("  {} (count={})", String::from_utf8_lossy(kmer), stats.observed_count);
        }
    }
    
    for &kmer_a in &kmers {
        for &kmer_b in &kmers {
            if kmer_a == kmer_b {
                continue;
            }
            
            let stats_a = non_homopolymer_kmers.get(kmer_a).unwrap();
            let stats_b = non_homopolymer_kmers.get(kmer_b).unwrap();
            
            // Check if counts are reasonably compatible
            if (stats_b.observed_count as f64) < (stats_a.observed_count as f64 * edge_join_threshold) ||
               (stats_a.observed_count as f64) < (stats_b.observed_count as f64 * edge_join_threshold) {
                #[cfg(test)]
                println!("  Skipping {} -> {} (count mismatch: {} vs {})", 
                         String::from_utf8_lossy(kmer_a), String::from_utf8_lossy(kmer_b),
                         stats_a.observed_count, stats_b.observed_count);
                continue;
            }
            
            if kmer_a[1..] == kmer_b[..k-1] {
                // Found a sliding overlap (length k-1)
                #[cfg(test)]
                println!("  Found edge: {} -> {}", 
                         String::from_utf8_lossy(kmer_a), String::from_utf8_lossy(kmer_b));

                graph.entry(kmer_a.clone()).or_default().push(kmer_b.clone());
                reverse_graph.entry(kmer_b.clone()).or_default().push(kmer_a.clone());
                // Continue to find all possible edges (don't break early)
            }
        }
    }

    // Find start nodes (nodes with no incoming edges)
    let start_nodes: Vec<_> = non_homopolymer_kmers.keys()
        .filter(|kmer| !reverse_graph.contains_key(*kmer))
        .collect();
    
    let mut assembled_sequences = HashMap::new();
    let mut processed = std::collections::HashSet::new();
    
    // Queue of potential start nodes. 
    // We start with the "true" start nodes (no incoming edges).
    // Then we will add unvisited nodes sorted by count.
    let mut start_node_queue = start_nodes;
    
    // Sort true start nodes by count (descending) to prioritize high abundance paths
    start_node_queue.sort_by(|a, b| {
        let count_a = non_homopolymer_kmers.get(*a).unwrap().observed_count;
        let count_b = non_homopolymer_kmers.get(*b).unwrap().observed_count;
        count_b.cmp(&count_a)
    });

    loop {
        // If queue is empty, look for any unvisited nodes to break cycles
        if start_node_queue.is_empty() {
            let mut remaining: Vec<_> = non_homopolymer_kmers.keys()
                .filter(|k| !processed.contains(*k))
                .collect();
            
            if remaining.is_empty() {
                break;
            }
            
            // Sort by count to pick the "best" place to break a cycle
            remaining.sort_by(|a, b| {
                let count_a = non_homopolymer_kmers.get(*a).unwrap().observed_count;
                let count_b = non_homopolymer_kmers.get(*b).unwrap().observed_count;
                count_b.cmp(&count_a)
            });
            
            // Take the top one as a new start node
            // We only take one at a time to see where it leads
            start_node_queue.push(remaining[0]);
        }

        let start_node = start_node_queue.remove(0);

        if processed.contains(start_node) {
            continue;
        }
        
        let mut path = vec![start_node.clone()];
        let mut current_node = start_node.clone();
        let mut visited_in_path = std::collections::HashSet::new();
        visited_in_path.insert(current_node.clone());
        processed.insert(current_node.clone());

        // Follow linear paths, stopping at branches or cycles
        while let Some(neighbors) = graph.get(&current_node) {
            // Only assemble non-diverging paths
            if neighbors.len() != 1 {
                #[cfg(test)]
                {
                    println!("  Divergence at {}: {} outgoing edges", 
                             String::from_utf8_lossy(&current_node), neighbors.len());
                    for neighbor in neighbors {
                        println!("    -> {}", String::from_utf8_lossy(neighbor));
                    }
                }
                break;
            }
            
            let next_node = &neighbors[0];
            
            // Cycle detection: if we've seen this node in the current path, stop
            if visited_in_path.contains(next_node) {
                #[cfg(test)]
                println!("  Cycle detected at {}", String::from_utf8_lossy(next_node));
                break;
            }
            
            path.push(next_node.clone());
            visited_in_path.insert(next_node.clone());
            processed.insert(next_node.clone());
            current_node = next_node.clone();
        }

        // Keep sequences with multiple k-mers, even if diverged
        // This captures the linear path up to the point of divergence
        if path.len() > 1 {
            // Reconstruct the sequence by finding actual overlaps
            let mut assembled_seq = path[0].clone();
            
            // Collect all k-mer counts for this assembly
            let kmer_counts: Vec<u64> = path.iter()
                .map(|kmer| non_homopolymer_kmers.get(kmer).unwrap().observed_count)
                .collect();
            
            for curr_kmer in &path[1..] {
                // Append the non-overlapping part of current k-mer
                assembled_seq.push(curr_kmer[k-1]);
            }
            
            // Use minimum count (most conservative estimate for primer abundance)
            let min_count = *kmer_counts.iter().min().unwrap();
            assembled_sequences.insert(assembled_seq, min_count);
        }
    }
    
    // Filter out sequences that are substrings of other assembled sequences
    // This handles cases where we started assembly from the middle of a sequence (suffix)
    // as well as the full sequence
    let keys: Vec<Vec<u8>> = assembled_sequences.keys().cloned().collect();
    let mut to_remove = std::collections::HashSet::new();
    
    for seq_a in &keys {
        for seq_b in &keys {
            if seq_a.len() < seq_b.len() && is_substring(seq_a, seq_b) {
                to_remove.insert(seq_a.clone());
                break;
            }
        }
    }
    
    for seq in to_remove {
        assembled_sequences.remove(&seq);
    }

    if !assembled_sequences.is_empty() {
        let lengths: Vec<usize> = assembled_sequences.keys().map(|s| s.len()).collect();
        let avg_length = lengths.iter().sum::<usize>() as f64 / lengths.len() as f64;
        let min_length = lengths.iter().min().unwrap();
        let max_length = lengths.iter().max().unwrap();
        
        println!("  Assembly found {} sequences (length: min={}, max={}, avg={:.1})", 
                 assembled_sequences.len(), min_length, max_length, avg_length);
    }
    assembled_sequences
}

// Reverse complement
//
/// Generic function to merge sequences with their reverse complements (version that handles stats)
/// Works for both k-mers and assembled sequences
fn merge_sequences_with_rc(
    sequences: HashMap<Vec<u8>, u64>,
    enable_merging: bool,
    total_length: usize,
) -> HashMap<Vec<u8>, MergeResult> {
    let mut results = HashMap::new();
    
    if !enable_merging {
        // No RC merging, convert to single results
        for (seq, count) in sequences {
            let stats = KmerStats::from_count(seq.clone(), count, total_length);
            results.insert(seq.clone(), MergeResult::single(seq, count, stats));
        }
        return results;
    }
    
    let mut processed = std::collections::HashSet::new();
    
    for (seq, count) in &sequences {
        if processed.contains(seq) {
            continue;
        }
        
        let rev_comp = reverse_complement(seq);
        
        // Check if we've already seen the reverse complement
        if let Some(rc_count) = sequences.get(&rev_comp) {
            if !processed.contains(&rev_comp) && seq != &rev_comp {
                // Choose the lexicographically smaller sequence as canonical
                let (canonical, forward_count, reverse_count) = if seq <= &rev_comp {
                    (seq.clone(), *count, *rc_count)
                } else {
                    (rev_comp.clone(), *rc_count, *count)
                };
                
                let total = forward_count + reverse_count;
                let stats = KmerStats::from_count(canonical.clone(), total, total_length);
                
                results.insert(
                    canonical.clone(),
                    MergeResult::merged(canonical, forward_count, reverse_count, stats),
                );
                
                processed.insert(seq.clone());
                processed.insert(rev_comp);
            }
        } else {
            // No reverse complement found, or it's a palindrome
            let stats = KmerStats::from_count(seq.clone(), *count, total_length);
            results.insert(seq.clone(), MergeResult::single(seq.clone(), *count, stats));
            processed.insert(seq.clone());
        }
    }
    
    results
}

fn merge_kmers_with_rc(
    enriched_kmers: &HashMap<Vec<u8>, KmerStats>,
    rc_merging_applied: bool,
    read_count: usize,
    total_length: usize,
    source: SequenceSource,
) -> Vec<EnrichedSequence> {
    // Convert KmerStats to counts for merging
    let kmer_counts: HashMap<Vec<u8>, u64> = enriched_kmers
        .iter()
        .map(|(k, stats)| (k.clone(), stats.observed_count))
        .collect();
    
    let merged_results = merge_sequences_with_rc(kmer_counts, rc_merging_applied, total_length);
    
    // Convert MergeResults to EnrichedSequences
    merged_results.into_values().map(|result| result.to_enriched_sequence(read_count, source.clone()))
        .collect()
}

/// Check if a k-mer is a substring of any assembled sequence or their reverse complements
/// This is needed when reverse complement merging has been applied to assembled sequences
fn is_substring_of_assembled_sequences(
    kmer: &[u8], 
    assembled_results: &HashMap<Vec<u8>, MergeResult>,
    check_reverse_complement: bool
) -> bool {
    for merge_result in assembled_results.values() {
        // Check forward direction
        if is_substring(kmer, &merge_result.sequence) {
            return true;
        }
        
        // Check reverse complement direction if requested
        if check_reverse_complement {
            let rc = reverse_complement(&merge_result.sequence);
            if is_substring(kmer, &rc) {
                return true;
            }
        }
    }
    false
}

/// Arguments for `write_report`.
struct WriteReportArgs<'a> {
    output_path: &'a Path,
    enriched_kmers: &'a HashMap<usize, HashMap<Vec<u8>, KmerStats>>,
    assembled_results: &'a HashMap<Vec<u8>, MergeResult>,
    k_min: usize,
    k_max: usize,
    rc_merging_applied: bool,
    total_length: usize,
    read_count: usize,
}

fn write_report(args: WriteReportArgs<'_>) -> Result<()> {
    let mut all_results: Vec<EnrichedSequence> = Vec::new();

    // Handle assembled sequences
    for merge_result in args.assembled_results.values() {
        // Filter out assembled sequences that are dirty homopolymers
        // Keep pure homopolymers (e.g. AAAAAA) but remove mixed ones (e.g. AAAAAAAG)
        if is_homopolymer(&merge_result.sequence) && !is_pure_homopolymer(&merge_result.sequence) {
            continue;
        }

        let enriched = merge_result.to_enriched_sequence(
            args.read_count,
            SequenceSource::Assembled { from_k: args.k_max },
        );
        all_results.push(enriched);
    }

    // Handle k-mers from k_max, with RC merging if applicable
    if let Some(enriched_k_max) = args.enriched_kmers.get(&args.k_max) {
        let mut kmer_results = merge_kmers_with_rc(
            enriched_k_max,
            args.rc_merging_applied,
            args.read_count,
            args.total_length,
            SequenceSource::Kmer(args.k_max),
        );

        // Filter out k-mers that are substrings of assembled sequences
        kmer_results.retain(|enriched| {
            let kmer = enriched.sequence.as_bytes();
            !is_substring_of_assembled_sequences(kmer, args.assembled_results, args.rc_merging_applied)
                && (!is_homopolymer(kmer) || is_pure_homopolymer(kmer))
        });

        all_results.extend(kmer_results);
    }

    // Handle k-mers from smaller k values, with RC merging if applicable
    for k in (args.k_min..args.k_max).rev() {
        if let Some(enriched_k) = args.enriched_kmers.get(&k) {
            let mut kmer_results = merge_kmers_with_rc(
                enriched_k,
                args.rc_merging_applied,
                args.read_count,
                args.total_length,
                SequenceSource::Kmer(k),
            );

            // Filter out k-mers that are substrings of assembled sequences
            kmer_results.retain(|enriched| {
                let kmer = enriched.sequence.as_bytes();
                !is_substring_of_assembled_sequences(kmer, args.assembled_results, args.rc_merging_applied)
                    && !is_homopolymer(kmer)
            });

            all_results.extend(kmer_results);
        }
    }

    // Sort by sqrt_deviance (descending)
    all_results.sort_by(|a, b| {
        b.sqrt_deviance
            .partial_cmp(&a.sqrt_deviance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // TODO: Filter enriched results for unique and non-overlapping sequences ?

    let mut writer = csv::Writer::from_path(args.output_path)?;
    writer.write_record([
        "sequence",
        "length",
        "estimated_count",
        "counts_per_read",
        "source_k",
        "sqrt_deviance",
        "log_fold_enrichment",
    ])?;

    for enriched in all_results {
        enriched.write_to_csv(&mut writer)?;
    }

    writer.flush()?;
    Ok(())
}

pub fn run(file_path: &Path, args: &KmerEnrichmentArgs) -> Result<()> {
    // Create configuration from arguments (includes validation)
    let config = KmerConfig::from_args(args)?;

    println!("Starting k-mer enrichment analysis...");

    // Collect k-values for processing
    let k_values = config.k_values();
    println!(
        "Parameters: k_values={:?}, top_kmers={}, filter_method={}",
        k_values,
        config.top_kmers,
        config.filter_method_description()
    );

    // Load sequences
    println!("Loading sequences...");
    let mut reader = SequenceReader::from_path(file_path)?;
    let (records, total_length) = if args.sample {
        // Reservoir sampling: uniformly sample `max_reads` reads from the entire file
        // using one pass and O(max_reads) memory.
        let reservoir_size = args.max_reads;
        let mut reservoir: Vec<SequenceRecord> = Vec::with_capacity(reservoir_size);
        let mut reservoir_total_length: usize = 0;

        // Fixed seed for debugging
        let mut rng = StdRng::seed_from_u64(2026);
        let mut index: usize = 0;
        while let Some(record) = reader.get_index(index)? {
            index += 1;

            if reservoir.len() < reservoir_size {
                reservoir_total_length += record.seq().len();
                reservoir.push(record);
                continue;
            }

            // Choose a random integer j in [0, index)
            // If j < reservoir_size, replace that element.
            let j = rng.gen_range(0..index);
            if j < reservoir_size {
                // Update total length accounting
                reservoir_total_length = reservoir_total_length
                    .saturating_sub(reservoir[j].seq().len())
                    .saturating_add(record.seq().len());
                reservoir[j] = record;
            }
        }

        (reservoir, reservoir_total_length)
    } else {
        // Take the first `max_reads` reads (or all if not set)
        let mut records = Vec::new();
        let mut total_length = 0;
        let mut index = 0;
        let mut remaining = if args.max_reads > 0 {
            Some(args.max_reads)
        } else {
            None 
        };
        while remaining.is_none_or(|n| n > 0) {
            let Some(record) = reader.get_index(index)? else { break };
            total_length += record.seq().len();
            records.push(record);
            index += 1;
            if let Some(n) = remaining.as_mut() {
                *n = n.saturating_sub(1);
            }
        }
        (records, total_length)
    };

    let read_count = records.len();
    println!("Loaded {read_count} sequences with total length {total_length} bp");

    // Load anti-reference records if requested
    let anti_reference_records = if let Some(ref anti_ref) = config.anti_reference_path {
        println!("Loading anti-reference records from {} ...", anti_ref.display());
        let recs = load_anti_reference_records(anti_ref)?;
        println!("  anti-reference: loaded {} records", recs.len());
        Some(recs)
    } else {
        None
    };

    // K-mer counting and top-N selection
    println!("K-mer counting and selection...");
    let enriched_kmers: HashMap<_, _> = k_values
        .par_iter()
        .map(|&k| {
            println!("Processing {k}-mers...");
            let kmer_stats = if let Some(min_count) = config.min_count {
                count_kmers_with_min_count(
                    &records, k,
                    (min_count * read_count as f64).ceil() as u64,
                    total_length
                )
            } else {
                count_kmers_with_zscore_stats(&records, k, total_length, config.z_score_threshold)
            };
            println!("Found {} {k}-mers above threshold", kmer_stats.len());

            let selected_kmers = select_top_kmers(kmer_stats, k, config.top_kmers,
                anti_reference_records.as_deref(),
                config.anti_reference_error_rate,
            );
            (k, selected_kmers)
        })
        .collect();

    // Cross-k substring filtering
    println!("Substring filtering...");
    let k_min = *k_values.first().unwrap();
    let k_max = *k_values.last().unwrap();
    let enriched_kmers = filter_substrings(enriched_kmers, config.substring_count_ratio_threshold);

    // Assembly
    let mut assembled_sequences = HashMap::new();
    if config.assemble {
        println!("Assembly ...");
        if let Some(enriched_k_max) = enriched_kmers.get(&k_max) {
            assembled_sequences = assemble_kmers(enriched_k_max, k_max, config.substring_count_ratio_threshold);
        }
    }

    // Optional post-assembly reverse complement merging
    let assembled_results = if config.detect_reverse_complement {
        println!("Merging reverse complement sequences...");
        let original_count = assembled_sequences.len();
        let merged_results = merge_sequences_with_rc(assembled_sequences, true, total_length);
        let merged_count = merged_results.len();
        if original_count != merged_count {
            println!("  Merged {original_count} sequences to {merged_count} after reverse complement consolidation");
        }
        merged_results
    } else {
        merge_sequences_with_rc(assembled_sequences, false, total_length)
    };

    write_report(WriteReportArgs {
        output_path: &config.output_path,
        enriched_kmers: &enriched_kmers,
        assembled_results: &assembled_results,
        k_min,
        k_max,
        rc_merging_applied: config.detect_reverse_complement,
        total_length,
        read_count,
    })?;
    println!("Results written to {}", config.output_path.display());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_bytes(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    #[test]
    fn test_is_homopolymer() {
        assert!(is_homopolymer(&to_bytes("AAAAAAAA")));
        assert!(is_homopolymer(&to_bytes("TTTTTTTT")));
        assert!(is_homopolymer(&to_bytes("GGGGGGGG")));
        assert!(is_homopolymer(&to_bytes("CCCCCCCC")));
        assert!(is_homopolymer(&to_bytes("AAAAAAAT"))); // 87.5% A, should be true
        assert!(!is_homopolymer(&to_bytes("ATCGATCG"))); // 25% each, should be false
        assert!(!is_homopolymer(&to_bytes("ATACCACTGC"))); // Mixed, should be false
    }

    #[test]
    fn test_filter_substrings_simple_removal() {
        let mut enriched = HashMap::new();
        let mut k8 = HashMap::new();
        k8.insert(to_bytes("AAAAAAAA"), KmerStats::new(to_bytes("AAAAAAAA"), 100, 50.0));
        let mut k9 = HashMap::new();
        k9.insert(to_bytes("AAAAAAAAA"), KmerStats::new(to_bytes("AAAAAAAAA"), 90, 25.0)); // 90 >= 100 * 0.8
        enriched.insert(8, k8);
        enriched.insert(9, k9);

        let filtered = filter_substrings(enriched, 0.8);
        assert!(filtered.get(&8).unwrap().is_empty());
        assert_eq!(filtered.get(&9).unwrap().len(), 1);
    }

    #[test]
    fn test_filter_substrings_no_removal() {
        let mut enriched = HashMap::new();
        let mut k8 = HashMap::new();
        // Use non-homopolymer sequences to avoid the threshold^4 rule
        k8.insert(to_bytes("ATCGATCG"), KmerStats::new(to_bytes("ATCGATCG"), 100, 50.0));
        let mut k9 = HashMap::new();
        k9.insert(to_bytes("ATCGATCGA"), KmerStats::new(to_bytes("ATCGATCGA"), 70, 25.0)); // 70 < 100 * 0.8
        enriched.insert(8, k8);
        enriched.insert(9, k9);

        let filtered = filter_substrings(enriched, 0.8);
        // Both should survive because count ratio is below threshold
        assert_eq!(filtered.get(&8).unwrap().len(), 1);
        assert_eq!(filtered.get(&9).unwrap().len(), 1);
    }

    #[test]
    fn test_assemble_kmers_simple_linear() {
        // Test simple linear assembly with k=6 to avoid short overlap issues
        // ATCGAT -> TCGATC -> CGATCG (overlaps: TCGAT, CGATC - not homopolymers)
        let mut kmers = HashMap::new();
        let k = 6;
        kmers.insert(to_bytes("ATCGAT"), KmerStats::new(to_bytes("ATCGAT"), 100, 50.0));
        kmers.insert(to_bytes("TCGATC"), KmerStats::new(to_bytes("TCGATC"), 95, 50.0));
        kmers.insert(to_bytes("CGATCG"), KmerStats::new(to_bytes("CGATCG"), 90, 50.0));
        
        let assembled = assemble_kmers(&kmers, k, 0.8);
        
        // Debug output
        println!("Assembled sequences: {} results", assembled.len());
        for (seq, count) in &assembled {
            println!("  {} (count={})", String::from_utf8_lossy(seq), count);
        }
        
        // Should assemble into "ATCGATCG" with min count of 90
        assert_eq!(assembled.len(), 1, "Expected 1 assembled sequence, got {}", assembled.len());
        assert!(assembled.contains_key(&to_bytes("ATCGATCG")), "Expected ATCGATCG but got: {:?}", 
                assembled.keys().map(|k| String::from_utf8_lossy(k).to_string()).collect::<Vec<_>>());
        assert_eq!(*assembled.get(&to_bytes("ATCGATCG")).unwrap(), 90); // minimum count
    }

    #[test]
    fn test_assemble_kmers_stops_at_branch() {
        // Test that assembly records sequence up to branching point with k=6
        // ATCGAT -> TCGATC -> {CGATCA, CGATCG} (branch at TCGATC)
        // Should assemble ATCGAT -> TCGATC and stop (producing "ATCGATC")
        let mut kmers = HashMap::new();
        let k = 6;
        kmers.insert(to_bytes("ATCGAT"), KmerStats::new(to_bytes("ATCGAT"), 100, 50.0));
        kmers.insert(to_bytes("TCGATC"), KmerStats::new(to_bytes("TCGATC"), 95, 50.0));
        kmers.insert(to_bytes("CGATCA"), KmerStats::new(to_bytes("CGATCA"), 90, 50.0));
        kmers.insert(to_bytes("CGATCG"), KmerStats::new(to_bytes("CGATCG"), 88, 50.0));
        
        let assembled = assemble_kmers(&kmers, k, 0.8);
        
        // Should assemble ATCGAT -> TCGATC, stopping at the branch and recording this path
        assert_eq!(assembled.len(), 1, "Expected 1 assembled sequence up to branch point");
        // print the assembled sequences for debugging
        for (seq, count) in &assembled {
            println!("  {} (count={})", String::from_utf8_lossy(seq), count);
        }
        assert!(assembled.contains_key(&to_bytes("ATCGATC")), 
                "Expected ATCGATC (assembled up to branch), got: {:?}",
                assembled.keys().map(|k| String::from_utf8_lossy(k).to_string()).collect::<Vec<_>>());
        // Count should be minimum of path (95 since ATCGAT=100, TCGATC=95)
        assert_eq!(*assembled.get(&to_bytes("ATCGATC")).unwrap(), 95);
    }

    #[test]
    fn test_assemble_kmers_count_mismatch() {
        // Test that k-mers with very different counts are not connected
        let mut kmers = HashMap::new();
        let k = 6;
        kmers.insert(to_bytes("ATCGAT"), KmerStats::new(to_bytes("ATCGAT"), 100, 50.0));
        kmers.insert(to_bytes("TCGATC"), KmerStats::new(to_bytes("TCGATC"), 50, 50.0)); // 50 < 100 * 0.8
        
        let assembled = assemble_kmers(&kmers, k, 0.8);
        
        // Should not assemble due to count mismatch
        assert_eq!(assembled.len(), 0);
    }

    #[test]
    fn test_assemble_kmers_multiple_paths_with_branches() {
        // Test with multiple paths: 
        // Path 1: GCTAGC -> CTAGCT -> TAGCTA (linear, should assemble fully to GCTAGCTA)
        // Path 2: ATCGAT -> TCGATC -> {CGATCA, CGATCG} (branch, should assemble to ATCGATC)
        let mut kmers = HashMap::new();
        let k = 6;
        
        // Linear path
        kmers.insert(to_bytes("GCTAGC"), KmerStats::new(to_bytes("GCTAGC"), 80, 50.0));
        kmers.insert(to_bytes("CTAGCT"), KmerStats::new(to_bytes("CTAGCT"), 82, 50.0));
        kmers.insert(to_bytes("TAGCTA"), KmerStats::new(to_bytes("TAGCTA"), 81, 50.0));
        
        // Branching path
        kmers.insert(to_bytes("ATCGAT"), KmerStats::new(to_bytes("ATCGAT"), 100, 50.0));
        kmers.insert(to_bytes("TCGATC"), KmerStats::new(to_bytes("TCGATC"), 95, 50.0));
        kmers.insert(to_bytes("CGATCA"), KmerStats::new(to_bytes("CGATCA"), 90, 50.0));
        kmers.insert(to_bytes("CGATCG"), KmerStats::new(to_bytes("CGATCG"), 88, 50.0));
        
        let assembled = assemble_kmers(&kmers, k, 0.8);
        
        println!("Assembled sequences: {}", assembled.len());
        for (seq, count) in &assembled {
            println!("  {} (count={})", String::from_utf8_lossy(seq), count);
        }
        
        // Should have 2 assembled sequences:
        // 1. GCTAGCTA (linear path, full assembly)
        // 2. ATCGATC (branching path, assembled up to branch point)
        assert_eq!(assembled.len(), 2, "Expected 2 assembled sequences");
        
        assert!(assembled.contains_key(&to_bytes("GCTAGCTA")), 
                "Expected GCTAGCTA from linear path");
        assert_eq!(*assembled.get(&to_bytes("GCTAGCTA")).unwrap(), 80); // minimum of 80,82,81
        
        assert!(assembled.contains_key(&to_bytes("ATCGATC")), 
                "Expected ATCGATC from branching path (up to divergence)");
        assert_eq!(*assembled.get(&to_bytes("ATCGATC")).unwrap(), 95); // minimum of 100,95
    }

    #[test]
    fn test_overlap_detection() {
        // Debug test to verify overlap detection works with longer k-mers
        let kmer_a = to_bytes("ATCGAT");
        let kmer_b = to_bytes("TCGATC");
        let k = 6;
        
        // Check if overlap exists: ATCGAT[1..] should be "TCGAT", TCGATC[..5] should be "TCGAT"
        assert_eq!(&kmer_a[1..], &kmer_b[..k-1]);
        
        // Check homopolymer status of overlap
        let overlap = &kmer_a[1..];
        println!("Overlap: {:?}", String::from_utf8_lossy(overlap));
        println!("Is homopolymer: {}", is_homopolymer(overlap));
        assert!(!is_homopolymer(overlap), "TCGAT should not be a homopolymer");
    }
}
