use crate::io::{SequenceReader, SequenceRecord};
use anyhow::Result;
use clap::Args;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::ops::Range;

const SUBSTRING_COUNT_RATIO_THRESHOLD: f64 = 0.8; // Threshold for substring filtering

/// Statistics for a k-mer including enrichment metrics
#[derive(Debug, Clone)]
pub struct KmerStats {
    pub sequence: Vec<u8>,
    pub observed_count: u64,
    pub expected_count: f64,
    pub neg_log10_pvalue: f64,
    pub sqrt_deviance: f64,
    pub log_fold_enrichment: f64,
}

impl KmerStats {
    /// Create new k-mer statistics
    fn new(
        sequence: Vec<u8>, 
        observed_count: u64, 
        expected_count: f64,
        // total_sequences: usize, // for multiple testing correction
    ) -> Self {
        
        let x = observed_count as f64 / expected_count;
        let h = x * x.ln() - x + 1.0;
        let neg_log10_pvalue = expected_count * h / std::f64::consts::LN_10;
        
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
        
        Self {
            sequence,
            observed_count,
            expected_count,
            neg_log10_pvalue,
            sqrt_deviance,
            log_fold_enrichment,
        }
    }
}

/// Result of merging sequences with their reverse complements
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub sequence: Vec<u8>,
    pub total_count: u64,
    pub forward_count: Option<u64>,  // Some if merged, None if single
    pub reverse_count: Option<u64>,  // Some if merged, None if single
    pub stats: Option<KmerStats>,    // Statistical information
}

impl MergeResult {
    /// Create a result for a single sequence (not merged)
    fn single(sequence: Vec<u8>, count: u64) -> Self {
        Self {
            sequence,
            total_count: count,
            forward_count: None,
            reverse_count: None,
            stats: None,
        }
    }
    
    /// Create a result for merged sequences
    fn merged(sequence: Vec<u8>, forward_count: u64, reverse_count: u64) -> Self {
        Self {
            sequence,
            total_count: forward_count + reverse_count,
            forward_count: Some(forward_count),
            reverse_count: Some(reverse_count),
            stats: None,
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
}

#[derive(Debug, Args, Clone)]
pub struct KmerEnrichmentArgs {
    /// Path to write the output CSV file.
    #[clap(short, long)]
    pub output: PathBuf,

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
    #[clap(long, default_value_t = 200)]
    pub top_kmers: usize,

    /// Minimum count threshold for k-mers (overrides z-score if provided).
    #[clap(long)]
    pub min_count: Option<u64>,

    /// Z-score threshold for k-mer enrichment (default: 5.0).
    #[clap(long, default_value_t = 5.0)]
    pub z_score_threshold: f64,

    /// Detect and merge reverse complement k-mers.
    #[clap(long)]
    pub detect_reverse_complement: bool,

}

/// Configuration struct for centralized parameter management
#[derive(Debug, Clone)]
pub struct KmerConfig {
    pub k_range: Range<usize>,
    pub k_step: usize,
    pub top_kmers: usize,
    pub z_score_threshold: f64,
    pub min_count: Option<u64>,
    pub output_path: PathBuf,
    pub detect_reverse_complement: bool,
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
            k_step: args.k_step,
            top_kmers: args.top_kmers,
            z_score_threshold: args.z_score_threshold,
            min_count: args.min_count,
            output_path,
            detect_reverse_complement: args.detect_reverse_complement,
        })
    }
    
    /// Get the k-values to process as an arithmetic sequence
    pub fn k_values(&self) -> Vec<usize> {
        (self.k_range.start..self.k_range.end)
            .step_by(self.k_step)
            .collect()
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
    if args.k_min > args.k_max {
        return Err(anyhow::anyhow!(
            "k-min ({}) must be less than or equal to k-max ({})",
            args.k_min, args.k_max
        ));
    }
    
    if args.k_step == 0 {
        return Err(anyhow::anyhow!("k-step must be greater than 0"));
    }
    
    if args.k_step > (args.k_max - args.k_min) {
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
    // not calculating adjusted p-values anymore
    // let total_kmers_count = kmer_counts.len();
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
    
    // For Poisson distribution: variance = mean
    let expected_std = expected_frequency.sqrt();
    let min_count_threshold = (expected_frequency + z_threshold * expected_std).ceil() as u64;
    
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
    // let total_kmers_count = kmer_counts.len();
    kmer_counts
        .into_iter()
        .filter(|(_, count)| *count >= min_count_threshold)
        .map(|(kmer, count)| {
            let stats = KmerStats::new(kmer.clone(), count, expected_frequency);
            (kmer, stats)
        })
        .collect()
}

/// Select top k-mers with balanced approach for homopolymers
fn select_top_kmers(
    kmer_stats: HashMap<Vec<u8>, KmerStats>, 
    k: usize, 
    top_n: usize,
) -> HashMap<Vec<u8>, KmerStats> {
    let initial_count = kmer_stats.len();
    
    if initial_count <= top_n {
        println!("  {initial_count} {k}-mers selected (keeping all)");
        return kmer_stats;
    }

    // Separate homopolymers from other sequences
    let (homopolymers, other_kmers): (Vec<_>, Vec<_>) = kmer_stats
        .iter()
        .partition(|(kmer, _)| is_homopolymer(kmer));

    // Strategy: Keep some homopolymers, but focus on non-homopolymer sequences
    let max_homopolymers = (top_n / 10).clamp(4, 20); // 10% for homopolymers, 4-20 range
    let remaining_slots = top_n.saturating_sub(max_homopolymers.min(homopolymers.len()));

    let mut result = HashMap::new();

    // Add top homopolymers by count
    let mut sorted_homopolymers = homopolymers;
    sorted_homopolymers.sort_by(|a, b| b.1.observed_count.cmp(&a.1.observed_count));
    for (kmer, stats) in sorted_homopolymers.into_iter().take(max_homopolymers) {
        result.insert(kmer.clone(), stats.clone());
    }

    // Add top non-homopolymer sequences
    let mut sorted_others: Vec<_> = other_kmers.into_iter().collect();
    sorted_others.sort_by(|a, b| b.1.observed_count.cmp(&a.1.observed_count));
    for (kmer, stats) in sorted_others.into_iter().take(remaining_slots) {
        result.insert(kmer.clone(), stats.clone());
    }
    
    let homo_count = result.iter().filter(|(kmer, _)| is_homopolymer(kmer)).count();
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
    _k_min: usize,
    k_max: usize,
) -> HashMap<usize, HashMap<Vec<u8>, KmerStats>> {
    // Get all k-values that are actually present (in case of step > 1)
    let mut k_values: Vec<usize> = enriched_kmers.keys().copied().collect();
    k_values.sort();
    
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
                                ratio >= SUBSTRING_COUNT_RATIO_THRESHOLD && is_sub
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

fn assemble_kmers(
    enriched_kmers: &HashMap<Vec<u8>, KmerStats>,
    k: usize,
) -> HashMap<Vec<u8>, u64> {
    if enriched_kmers.is_empty() {
        return HashMap::new();
    }

    // Build overlap graph with stricter overlap criteria
    let mut graph: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
    let mut reverse_graph: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();

    let kmers: Vec<_> = enriched_kmers.keys().collect();
    
    // Find valid overlaps between k-mers
    // Only consider overlaps of at least k/2 length to avoid spurious connections
    let min_overlap = (k / 2).max(3);
    
    for &kmer_a in &kmers {
        for &kmer_b in &kmers {
            if kmer_a == kmer_b {
                continue;
            }
            
            let stats_a = enriched_kmers.get(kmer_a).unwrap();
            let stats_b = enriched_kmers.get(kmer_b).unwrap();
            
            // Check if counts are reasonably compatible
            // Allow more variation but still require some similarity
            if (stats_b.observed_count as f64) < (stats_a.observed_count as f64 * 0.3) || 
               (stats_b.observed_count as f64) > (stats_a.observed_count as f64 * 3.0) {
                continue;
            }
            
            // Find significant overlaps: kmer_a suffix matches kmer_b prefix
            let max_overlap = (kmer_a.len() - 1).min(kmer_b.len() - 1);
            for overlap_len in (min_overlap..=max_overlap).rev() {
                let suffix = &kmer_a[kmer_a.len() - overlap_len..];
                let prefix = &kmer_b[..overlap_len];
                
                if suffix == prefix {
                    // Found a valid significant overlap
                    graph.entry(kmer_a.clone()).or_default().push(kmer_b.clone());
                    reverse_graph.entry(kmer_b.clone()).or_default().push(kmer_a.clone());
                    break; // Take the longest overlap
                }
            }
        }
    }

    // Find start nodes (nodes with no incoming edges or weak incoming edges)
    let mut start_nodes: Vec<_> = enriched_kmers.keys()
        .filter(|kmer| !reverse_graph.contains_key(*kmer))
        .collect();
    
    // If no clear start nodes, pick nodes with highest counts as potential starts
    if start_nodes.is_empty() {
        let mut kmer_counts: Vec<_> = enriched_kmers.iter().collect();
        kmer_counts.sort_by(|a, b| b.1.observed_count.cmp(&a.1.observed_count));
        start_nodes = kmer_counts.into_iter().take(5).map(|(k, _)| k).collect();
    }

    let mut assembled_sequences = HashMap::new();
    let mut processed = std::collections::HashSet::new();

    for start_node in start_nodes {
        if processed.contains(start_node) {
            continue;
        }

        let mut path = vec![start_node.clone()];
        let mut current_node = start_node.clone();
        processed.insert(current_node.clone());

        // Follow linear paths, being more permissive about branching
        while let Some(neighbors) = graph.get(&current_node) {
            // Pick the best next neighbor (highest count among unprocessed)
            let mut best_next = None;
            let mut best_count = 0;
            
            for next_node in neighbors {
                if !processed.contains(next_node) {
                    let count = enriched_kmers.get(next_node).unwrap().observed_count;
                    if count > best_count {
                        best_count = count;
                        best_next = Some(next_node);
                    }
                }
            }
            
            if let Some(next_node) = best_next {
                path.push(next_node.clone());
                processed.insert(next_node.clone());
                current_node = next_node.clone();
            } else {
                break;
            }
        }

        // Keep sequences with multiple k-mers
        if path.len() > 1 {
            // Reconstruct the sequence by finding actual overlaps
            let mut assembled_seq = path[0].clone();
            let mut total_count = enriched_kmers.get(&path[0]).unwrap().observed_count;
            
            for i in 1..path.len() {
                let prev_kmer = &path[i-1];
                let curr_kmer = &path[i];
                
                // Find the overlap length between consecutive k-mers
                let max_overlap = (prev_kmer.len() - 1).min(curr_kmer.len() - 1);
                let mut overlap_len = 1;
                
                for ol in (min_overlap..=max_overlap).rev() {
                    let suffix = &prev_kmer[prev_kmer.len() - ol..];
                    let prefix = &curr_kmer[..ol];
                    if suffix == prefix {
                        overlap_len = ol;
                        break;
                    }
                }
                
                // Append the non-overlapping part of current k-mer
                assembled_seq.extend_from_slice(&curr_kmer[overlap_len..]);
                total_count += enriched_kmers.get(curr_kmer).unwrap().observed_count;
            }
            
            let avg_count = total_count / path.len() as u64;
            assembled_sequences.insert(assembled_seq, avg_count);
        }
    }
    
    if !assembled_sequences.is_empty() {
        println!("  Assembly found {} sequences", assembled_sequences.len());
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
) -> HashMap<Vec<u8>, MergeResult> {
    let mut results = HashMap::new();
    
    if !enable_merging {
        // No RC merging, convert to single results
        for (seq, count) in sequences {
            results.insert(seq.clone(), MergeResult::single(seq, count));
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
                
                results.insert(
                    canonical.clone(),
                    MergeResult::merged(canonical, forward_count, reverse_count),
                );
                
                processed.insert(seq.clone());
                processed.insert(rev_comp);
            }
        } else {
            // No reverse complement found, or it's a palindrome
            results.insert(seq.clone(), MergeResult::single(seq.clone(), *count));
            processed.insert(seq.clone());
        }
    }
    
    results
}

fn merge_kmers_with_rc(
    enriched_kmers: &HashMap<Vec<u8>, KmerStats>,
    rc_merging_applied: bool,
) -> Vec<(String, usize, String, String, f64, f64, f64)> {
    // Convert KmerStats to counts for merging, preserving the original stats
    let kmer_counts: HashMap<Vec<u8>, u64> = enriched_kmers
        .iter()
        .map(|(k, stats)| (k.clone(), stats.observed_count))
        .collect();
    
    let merged_results = merge_sequences_with_rc(kmer_counts, rc_merging_applied);
    
    let mut results = Vec::new();
    for (seq, merge_result) in merged_results {
        // Get the original stats for this sequence (or its reverse complement)
        let original_stats = enriched_kmers.get(&seq)
            .or_else(|| {
                let rev_comp = reverse_complement(&seq);
                enriched_kmers.get(&rev_comp)
            });
        
        let (neg_log10_pvalue, sqrt_deviance, log_fold_enrichment) = if let Some(stats) = original_stats {
            (stats.neg_log10_pvalue, stats.sqrt_deviance, stats.log_fold_enrichment)
        } else {
            // Fallback for merged sequences - calculate basic stats
            (0.0, 0.0, 0.0)
        };
        
        results.push((
            String::from_utf8_lossy(&merge_result.sequence).into_owned(),
            merge_result.sequence.len(),
            merge_result.format_count(),
            "".to_string(), // source_k will be filled by caller
            neg_log10_pvalue,
            sqrt_deviance,
            log_fold_enrichment,
        ));
    }
    
    results
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

/// Calculate statistics for assembled sequences based on their estimated count
fn calculate_assembled_stats(sequence: &[u8], estimated_count: u64, total_length: usize) -> (f64, f64, f64) {
    let k = sequence.len();
    let total_possible_kmers = total_length.saturating_sub(k - 1);
    let expected_count = total_possible_kmers as f64 / (4_f64.powi(k as i32));
    
    // For assembled sequences, we use a smaller multiple testing correction factor
    // since there are fewer assembled sequences than individual k-mers
    let x = estimated_count as f64 / expected_count;
    let h = x * x.ln() - x + 1.0;
    let neg_log10_pvalue = estimated_count as f64 * h / std::f64::consts::LN_10;
    
    // Calculate square root deviance
    let sqrt_deviance = if estimated_count as f64 > expected_count {
        let deviance = 2.0 * (estimated_count as f64 * (estimated_count as f64 / expected_count).ln() - 
                             (estimated_count as f64 - expected_count));
        deviance.sqrt()
    } else {
        0.0
    };
    
    // Calculate log fold enrichment
    let log_fold_enrichment = if expected_count > 0.0 {
        (estimated_count as f64 / expected_count).log2()
    } else {
        f64::INFINITY
    };
    
    (neg_log10_pvalue, sqrt_deviance, log_fold_enrichment)
}

fn write_report(
    output_path: &Path,
    enriched_kmers: &HashMap<usize, HashMap<Vec<u8>, KmerStats>>,
    assembled_results: &HashMap<Vec<u8>, MergeResult>,
    k_min: usize,
    k_max: usize,
    rc_merging_applied: bool,
    total_length: usize,
) -> Result<()> {
    let mut all_results = Vec::new();

    // Handle assembled sequences with consistent formatting
    for merge_result in assembled_results.values() {
        // Calculate statistics for assembled sequences based on their estimated count
        let (neg_log10_pvalue, sqrt_deviance, log_fold_enrichment) = 
            calculate_assembled_stats(&merge_result.sequence, merge_result.total_count, total_length);
        
        all_results.push((
            String::from_utf8_lossy(&merge_result.sequence).into_owned(),
            merge_result.sequence.len(),
            merge_result.format_count(),
            format!("assembled from k={k_max}"),
            neg_log10_pvalue,
            sqrt_deviance,
            log_fold_enrichment,
        ));
    }

    // Handle k-mers from k_max, with RC merging if applicable
    if let Some(enriched_k_max) = enriched_kmers.get(&k_max) {
        let mut kmer_results = merge_kmers_with_rc(enriched_k_max, rc_merging_applied);
        
        // Filter out k-mers that are substrings of assembled sequences
        kmer_results.retain(|(kmer_str, _, _, _, _, _, _)| {
            let kmer = kmer_str.as_bytes();
            !is_substring_of_assembled_sequences(kmer, assembled_results, rc_merging_applied)
        });
        
        // Set the source_k for these results
        for (_, _, _, source_k, _, _, _) in kmer_results.iter_mut() {
            *source_k = k_max.to_string();
        }
        
        all_results.extend(kmer_results);
    }

    // Handle k-mers from smaller k values, with RC merging if applicable
    for k in (k_min..k_max).rev() {
        if let Some(enriched_k) = enriched_kmers.get(&k) {
            let mut kmer_results = merge_kmers_with_rc(enriched_k, rc_merging_applied);
            
            // Filter out k-mers that are substrings of assembled sequences
            kmer_results.retain(|(kmer_str, _, _, _, _, _, _)| {
                let kmer = kmer_str.as_bytes();
                !is_substring_of_assembled_sequences(kmer, assembled_results, rc_merging_applied)
            });
            
            // Set the source_k for these results
            for (_, _, _, source_k, _, _, _) in kmer_results.iter_mut() {
                *source_k = k.to_string();
            }
            
            all_results.extend(kmer_results);
        }
    }

    // Sort by sqrt_deviance
    all_results.sort_by(|a, b| {
        let dev_a = a.5;
        let dev_b = b.5;
        dev_b.partial_cmp(&dev_a).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut writer = csv::Writer::from_path(output_path)?;
    writer.write_record([
        "sequence", 
        "length", 
        "estimated_count", 
        "source_k",
        "sqrt_deviance", 
        "log_fold_enrichment"
    ])?;
    
    for (sequence, length, count, source_k, _, sqrt_deviance, log_fold_enrichment) in all_results {
        // neg_log10_pvalue too large, not very useful
        let formatted_log_fold = if log_fold_enrichment.is_infinite() {
            if log_fold_enrichment.is_sign_positive() {
                "inf".to_string()
            } else {
                "-inf".to_string()
            }
        } else {
            format!("{log_fold_enrichment:.1}")
        };
        
        writer.write_record(&[
            sequence, 
            length.to_string(), 
            count, 
            source_k,
            format!("{sqrt_deviance:.1}"),
            formatted_log_fold,
        ])?;
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
    let mut records = Vec::new();
    let mut total_length = 0;
    let mut index = 0;
    while let Some(record) = reader.get_index(index)? {
        total_length += record.seq().len();
        records.push(record);
        index += 1;
    }
    println!("Loaded {} sequences with total length {} bp", records.len(), total_length);

    // K-mer counting and top-N selection
    println!("K-mer counting and selection...");
    let mut enriched_kmers = HashMap::new();
    for &k in &k_values {
        println!("Processing {k}-mers...");
        let kmer_stats = if let Some(min_count) = config.min_count {
            count_kmers_with_min_count(&records, k, min_count, total_length)
        } else {
            count_kmers_with_zscore_stats(&records, k, total_length, config.z_score_threshold)
        };
        println!("Found {} {k}-mers above threshold", kmer_stats.len());
        
        let selected_kmers = select_top_kmers(kmer_stats, k, config.top_kmers);
        enriched_kmers.insert(k, selected_kmers);
    }

    // Cross-k substring filtering
    println!("Substring filtering...");
    let k_min = *k_values.first().unwrap();
    let k_max = *k_values.last().unwrap();
    let enriched_kmers = filter_substrings(enriched_kmers, k_min, k_max);

    // Assembly and reporting
    println!("Assembly and reporting...");
    let mut assembled_sequences = HashMap::new();
    if let Some(enriched_k_max) = enriched_kmers.get(&k_max) {
        assembled_sequences = assemble_kmers(enriched_k_max, k_max);
    }

    // Optional post-assembly reverse complement merging
    let assembled_results = if config.detect_reverse_complement {
        println!("Merging reverse complement sequences...");
        let original_count = assembled_sequences.len();
        let merged_results = merge_sequences_with_rc(assembled_sequences, true);
        let merged_count = merged_results.len();
        if original_count != merged_count {
            println!("  Merged {original_count} sequences to {merged_count} after reverse complement consolidation");
        }
        merged_results
    } else {
        merge_sequences_with_rc(assembled_sequences, false)
    };

    write_report(&config.output_path, &enriched_kmers, &assembled_results, k_min, k_max, config.detect_reverse_complement, total_length)?;
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

        let filtered = filter_substrings(enriched, 8, 9);
        assert!(filtered.get(&8).unwrap().is_empty());
        assert_eq!(filtered.get(&9).unwrap().len(), 1);
    }

    #[test]
    fn test_filter_substrings_no_removal() {
        let mut enriched = HashMap::new();
        let mut k8 = HashMap::new();
        k8.insert(to_bytes("AAAAAAAA"), KmerStats::new(to_bytes("AAAAAAAA"), 100, 50.0));
        let mut k9 = HashMap::new();
        k9.insert(to_bytes("AAAAAAAAA"), KmerStats::new(to_bytes("AAAAAAAAA"), 70, 25.0)); // 70 < 100 * 0.8
        enriched.insert(8, k8);
        enriched.insert(9, k9);

        let filtered = filter_substrings(enriched, 8, 9);
        assert_eq!(filtered.get(&8).unwrap().len(), 1);
        assert_eq!(filtered.get(&9).unwrap().len(), 1);
    }
}
