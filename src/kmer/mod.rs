use crate::io::{SequenceReader, SequenceRecord};
use anyhow::Result;
use clap::Parser;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const SUBSTRING_COUNT_RATIO_THRESHOLD: f64 = 0.8; // Threshold for substring filtering

#[derive(Debug, Parser)]
pub struct KmerEnrichmentArgs {
    /// Path to write the output CSV file.
    #[clap(short, long)]
    pub output: PathBuf,

    /// Minimum k-mer length to check.
    #[clap(long, default_value_t = 8)]
    pub k_min: usize,

    /// Maximum k-mer length to check.
    #[clap(long, default_value_t = 25)]
    pub k_max: usize,

    /// Number of top k-mers to keep per k value.
    #[clap(long, default_value_t = 200)]
    pub top_kmers: usize,

    /// Minimum count threshold for k-mers (overrides z-score if provided).
    #[clap(long)]
    pub min_count: Option<u64>,

    /// Z-score threshold for k-mer enrichment (default: 5.0).
    #[clap(long, default_value_t = 5.0)]
    pub z_score_threshold: f64,
}

pub fn run(file_path: &Path, args: &KmerEnrichmentArgs) -> Result<()> {
    println!("Starting k-mer enrichment analysis...");
    
    // Determine which filtering method to use
    let filter_method = if let Some(min_count) = args.min_count {
        format!("min_count={min_count}")
    } else {
        format!("z_score_threshold={}", args.z_score_threshold)
    };
    
    println!("Parameters: k_min={}, k_max={}, top_kmers={}, filter_method={filter_method}", 
        args.k_min, args.k_max, args.top_kmers);

    // Phase 1: Load sequences
    println!("Phase 1: Loading sequences...");
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

    // Phase 2: K-mer counting and top-N selection
    println!("Phase 2: K-mer counting and selection...");
    let mut enriched_kmers = HashMap::new();
    for k in args.k_min..=args.k_max {
        println!("Processing {k}-mers...");
        let kmer_counts = if let Some(min_count) = args.min_count {
            count_kmers_with_min_count(&records, k, min_count)
        } else {
            count_kmers_with_zscore(&records, k, total_length, args.z_score_threshold)
        };
        println!("Found {} {k}-mers above threshold", kmer_counts.len());
        
        let selected_kmers = select_top_kmers(kmer_counts, k, args.top_kmers);
        enriched_kmers.insert(k, selected_kmers);
    }

    // Phase 3: Cross-k substring filtering
    println!("Phase 3: Substring filtering...");
    let enriched_kmers = filter_substrings(enriched_kmers, args.k_min, args.k_max);

    // Phase 4: Assembly and reporting
    println!("Phase 4: Assembly and reporting...");
    let mut assembled_sequences = HashMap::new();
    if let Some(enriched_k_max) = enriched_kmers.get(&args.k_max) {
        assembled_sequences = assemble_kmers(enriched_k_max, args.k_max);
    }

    write_report(&args.output, &enriched_kmers, &assembled_sequences, args.k_min, args.k_max)?;
    println!("Analysis complete. Results written to {}", args.output.display());

    Ok(())
}

fn count_kmers_with_min_count(records: &[SequenceRecord], k: usize, min_threshold: u64) -> HashMap<Vec<u8>, u64> {
    records
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
        })
        .into_iter()
        .filter(|(_, count)| *count >= min_threshold)
        .collect()
}

fn count_kmers_with_zscore(
    records: &[SequenceRecord], 
    k: usize, 
    total_length: usize,
    z_threshold: f64
) -> HashMap<Vec<u8>, u64> {
    // Calculate expected frequency for random k-mers
    // For equal probability of 4 bases: P(specific k-mer) = (1/4)^k
    // Expected count = total_possible_kmers * P(specific k-mer)
    let total_possible_kmers = total_length.saturating_sub(k - 1);
    let expected_frequency = total_possible_kmers as f64 / (4_f64.powi(k as i32));
    
    // For Poisson distribution: variance = mean
    let expected_std = expected_frequency.sqrt();
    let min_count_threshold = (expected_frequency + z_threshold * expected_std).ceil() as u64;
    
    println!("  Expected frequency: {expected_frequency:.2}, std: {expected_std:.2}, min_count (Z≥{z_threshold:.1}): {min_count_threshold}");
    
    // Count k-mers and filter by Z-score threshold
    records
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
        })
        .into_iter()
        .filter(|(_, count)| *count >= min_count_threshold)
        .collect()
}

/// Select top k-mers with balanced approach for homopolymers
fn select_top_kmers(
    kmer_counts: HashMap<Vec<u8>, u64>, 
    k: usize, 
    top_n: usize,
) -> HashMap<Vec<u8>, u64> {
    let initial_count = kmer_counts.len();
    
    if initial_count <= top_n {
        println!("  {initial_count} {k}-mers selected (keeping all)");
        return kmer_counts;
    }

    // Separate homopolymers from other sequences
    let (homopolymers, other_kmers): (Vec<_>, Vec<_>) = kmer_counts
        .iter()
        .partition(|(kmer, _)| is_homopolymer(kmer));

    // Strategy: Keep some homopolymers, but focus on non-homopolymer sequences
    let max_homopolymers = (top_n / 10).clamp(4, 20); // 10% for homopolymers, 4-20 range
    let remaining_slots = top_n.saturating_sub(max_homopolymers.min(homopolymers.len()));

    let mut result = HashMap::new();

    // Add top homopolymers by count
    let mut sorted_homopolymers = homopolymers;
    sorted_homopolymers.sort_by(|a, b| b.1.cmp(a.1));
    for (kmer, count) in sorted_homopolymers.into_iter().take(max_homopolymers) {
        result.insert(kmer.clone(), *count);
    }

    // Add top non-homopolymer sequences
    let mut sorted_others: Vec<_> = other_kmers.into_iter().collect();
    sorted_others.sort_by(|a, b| b.1.cmp(a.1));
    for (kmer, count) in sorted_others.into_iter().take(remaining_slots) {
        result.insert(kmer.clone(), *count);
    }
    
    let homo_count = result.iter().filter(|(kmer, _)| is_homopolymer(kmer)).count();
    let other_count = result.len() - homo_count;
    println!("  {} {k}-mers selected ({homo_count} homopolymers, {other_count} others)", 
        result.len());
    
    result
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

fn filter_substrings(
    mut enriched_kmers: HashMap<usize, HashMap<Vec<u8>, u64>>,
    k_min: usize,
    k_max: usize,
) -> HashMap<usize, HashMap<Vec<u8>, u64>> {
    // Process in increasing k order: k_min to k_max-1
    // This ensures shorter k-mers are filtered out by longer ones iteratively
    for k in k_min..k_max {
        let short_kmers = if let Some(short_kmers) = enriched_kmers.get(&k) {
            short_kmers.clone()
        } else {
            continue;
        };

        let original_len = short_kmers.len();
        
        let surviving_kmers: HashMap<Vec<u8>, u64> = short_kmers
            .into_par_iter()
            .filter(|(kmer_short, count_short)| {
                // Check against ALL longer k-mers (k+1, k+2, ..., k_max)
                let should_remove = (k+1..=k_max).any(|long_k| {
                    if let Some(long_kmers) = enriched_kmers.get(&long_k) {
                        long_kmers.iter().any(|(kmer_long, count_long)| {
                            let ratio = *count_long as f64 / *count_short as f64;
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

fn is_substring(sub: &[u8], main: &[u8]) -> bool {
    main.windows(sub.len()).any(|window| window == sub)
}

fn assemble_kmers(
    enriched_kmers: &HashMap<Vec<u8>, u64>,
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
            
            let count_a = enriched_kmers.get(kmer_a).unwrap();
            let count_b = enriched_kmers.get(kmer_b).unwrap();
            
            // Check if counts are reasonably compatible
            // Allow more variation but still require some similarity
            if (*count_b as f64) < (*count_a as f64 * 0.3) || (*count_b as f64) > (*count_a as f64 * 3.0) {
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
        kmer_counts.sort_by(|a, b| b.1.cmp(a.1));
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
                    let count = *enriched_kmers.get(next_node).unwrap();
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
            let mut total_count = *enriched_kmers.get(&path[0]).unwrap();
            
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
                total_count += enriched_kmers.get(curr_kmer).unwrap();
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

fn write_report(
    output_path: &Path,
    enriched_kmers: &HashMap<usize, HashMap<Vec<u8>, u64>>,
    assembled_sequences: &HashMap<Vec<u8>, u64>,
    k_min: usize,
    k_max: usize,
) -> Result<()> {
    let mut all_results = Vec::new();

    for (seq, count) in assembled_sequences.iter() {
        all_results.push((
            String::from_utf8_lossy(seq).into_owned(),
            seq.len(),
            *count,
            format!("assembled from k={k_max}"),
        ));
    }

    if let Some(enriched_k_max) = enriched_kmers.get(&k_max) {
        for (kmer, count) in enriched_k_max.iter() {
            if !assembled_sequences.keys().any(|ak| is_substring(kmer, ak)) {
                all_results.push((
                    String::from_utf8_lossy(kmer).into_owned(),
                    kmer.len(),
                    *count,
                    k_max.to_string(),
                ));
            }
        }
    }

    for k in (k_min..k_max).rev() {
        if let Some(enriched_k) = enriched_kmers.get(&k) {
            for (kmer, count) in enriched_k.iter() {
                if !assembled_sequences.keys().any(|ak| is_substring(kmer, ak)) {
                    all_results.push((
                        String::from_utf8_lossy(kmer).into_owned(),
                        kmer.len(),
                        *count,
                        k.to_string(),
                    ));
                }
            }
        }
    }

    all_results.sort_by(|a, b| b.2.cmp(&a.2));

    let mut writer = csv::Writer::from_path(output_path)?;
    writer.write_record(["sequence", "length", "estimated_count", "source_k"])?;
    for (sequence, length, count, source_k) in all_results {
        writer.write_record(&[sequence, length.to_string(), count.to_string(), source_k])?;
    }
    writer.flush()?;
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
        k8.insert(to_bytes("AAAAAAAA"), 100);
        let mut k9 = HashMap::new();
        k9.insert(to_bytes("AAAAAAAAA"), 90); // 90 >= 100 * 0.8
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
        k8.insert(to_bytes("AAAAAAAA"), 100);
        let mut k9 = HashMap::new();
        k9.insert(to_bytes("AAAAAAAAA"), 70); // 70 < 100 * 0.8
        enriched.insert(8, k8);
        enriched.insert(9, k9);

        let filtered = filter_substrings(enriched, 8, 9);
        assert_eq!(filtered.get(&8).unwrap().len(), 1);
        assert_eq!(filtered.get(&9).unwrap().len(), 1);
    }
}
