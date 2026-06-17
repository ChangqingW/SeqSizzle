use crate::app::{App, SearchPattern};
use crate::io::SequenceRecord;
use std::collections::HashMap;
use std::collections::VecDeque;

#[cfg(test)]
use ratatui::prelude::Color;

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum ReadParts {
    Match(SearchPattern),
    Space,         // could be Space(usize) to indicate length
    NegativeSpace, // indicate two matches are overlapped
}
impl std::fmt::Display for ReadParts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadParts::Match(x) => write!(f, "{}", x.search_string),
            ReadParts::Space => write!(f, ".."),
            ReadParts::NegativeSpace => write!(f, "-"),
        }
    }
}

/// Categorise a read
fn categorise_read(record: &SequenceRecord, search_patterns: &[SearchPattern]) -> Vec<ReadParts> {
    // merge overlapping intervals
    fn merge_overlap(mut intervals: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
        intervals.sort_by_key(|x| x.0);
        let mut ret: Vec<(usize, usize)> = Vec::new();
        for current in intervals {
            if ret.is_empty() {
                ret.push(current);
            } else {
                let last = ret.last_mut().unwrap();
                if current.0 <= last.1 {
                    last.1 = current.1.max(last.1);
                } else {
                    ret.push(current);
                }
            }
        }
        ret
    }

    // matched regions for each pattern as an IntervalSet
    let mut matches: Vec<VecDeque<(usize, usize)>> = search_patterns
        .iter()
        .map(|x| VecDeque::from(merge_overlap(App::search(record, x))))
        .collect();

    let mut ret: Vec<ReadParts> =
        Vec::with_capacity(matches.iter().map(|x| x.len()).sum::<usize>() * 2 + 1);
    let mut last_end: usize = 0;
    while matches.iter().any(|x| !x.is_empty()) {
        // pop the match with the lowest start position
        let (mut min_start, mut min_index) = (usize::MAX, usize::MAX);
        for (i, m) in matches.iter().enumerate() {
            if !m.is_empty() && m[0].0 < min_start {
                min_start = m[0].0;
                min_index = i;
            }
        }

        if min_start > last_end + 1 {
            ret.push(ReadParts::Space);
        } else if min_start <= last_end && last_end > 0 {
            ret.push(ReadParts::NegativeSpace);
        }
        ret.push(ReadParts::Match(search_patterns[min_index].clone()));
        last_end = matches[min_index][0].1;
        matches[min_index].remove(0);
    }

    if last_end + 1 < record.seq().len() {
        ret.push(ReadParts::Space);
    }

    ret
}

#[test]
fn test_categorise_read() {
    let sequence_record = SequenceRecord::Fasta {
        id: "id".to_string(),
        description: None,
        seq: b"ATCGCCATCGCCATCGCCATCGATCAAATCGGATC".to_vec(),
    };
    let patterns = vec![
        SearchPattern::new(String::from("ATCG"), Color::Red, 0, ""),
        SearchPattern::new(String::from("GATC"), Color::Red, 0, ""),
    ];
    let mut result = String::new();
    for i in categorise_read(&sequence_record, &patterns) {
        match i {
            ReadParts::Match(x) => result.push_str(x.search_string.as_str()),
            ReadParts::Space => result.push_str(".."),
            ReadParts::NegativeSpace => result.push('-'),
        }
    }
    assert_eq!(
        result,
        String::from("ATCG..ATCG..ATCG..ATCG-GATC..ATCGGATC")
    );
}

#[cfg(test)]
fn test_pattern(s: &str) -> SearchPattern {
    SearchPattern::new(String::from(s), Color::Red, 0, "")
}

#[test]
fn test_trim_flanking_space() {
    let a = test_pattern("AAAA");
    let b = test_pattern("CCCC");
    let flanked = vec![
        ReadParts::Space,
        ReadParts::Match(a.clone()),
        ReadParts::Space,
        ReadParts::Match(b.clone()),
        ReadParts::Space,
    ];
    let trimmed = trim_flanking_space(&flanked);
    assert_eq!(
        trimmed,
        &[
            ReadParts::Match(a.clone()),
            ReadParts::Space,
            ReadParts::Match(b.clone()),
        ]
    );

    // No flanking space: unchanged.
    let bare = vec![ReadParts::Match(a), ReadParts::Space, ReadParts::Match(b)];
    assert_eq!(trim_flanking_space(&bare), bare.as_slice());

    // All space: empty.
    assert!(trim_flanking_space(&[ReadParts::Space, ReadParts::Space]).is_empty());
}

#[test]
fn test_is_contained() {
    let a = test_pattern("AAAA");
    let b = test_pattern("CCCC");
    let c = test_pattern("GGGG");
    let d = test_pattern("TTTT");

    let shorter = vec![
        ReadParts::Match(a.clone()),
        ReadParts::Space,
        ReadParts::Match(b.clone()),
        ReadParts::Space,
        ReadParts::Match(c.clone()),
    ];

    // Prefix block.
    let longer_prefix = vec![
        ReadParts::Match(a.clone()),
        ReadParts::Space,
        ReadParts::Match(b.clone()),
        ReadParts::Space,
        ReadParts::Match(c.clone()),
        ReadParts::Space,
        ReadParts::Match(d.clone()),
    ];
    assert!(is_contained(&shorter, &longer_prefix));

    // Suffix block.
    let longer_suffix = vec![
        ReadParts::Match(d.clone()),
        ReadParts::Space,
        ReadParts::Match(a.clone()),
        ReadParts::Space,
        ReadParts::Match(b.clone()),
        ReadParts::Space,
        ReadParts::Match(c.clone()),
    ];
    assert!(is_contained(&shorter, &longer_suffix));

    // Separator mismatch (`..` vs `-`): not contained.
    let longer_neg = vec![
        ReadParts::Match(a.clone()),
        ReadParts::NegativeSpace,
        ReadParts::Match(b.clone()),
        ReadParts::Space,
        ReadParts::Match(c.clone()),
        ReadParts::Space,
        ReadParts::Match(d.clone()),
    ];
    assert!(!is_contained(&shorter, &longer_neg));

    // Subsequence with a gap (a..c) is not a contiguous block of a..b..c..d.
    let gapped = vec![
        ReadParts::Match(a.clone()),
        ReadParts::Space,
        ReadParts::Match(c.clone()),
    ];
    assert!(!is_contained(&gapped, &longer_prefix));

    // Equal / identical length never counts as contained.
    assert!(!is_contained(&longer_prefix, &longer_prefix));
}

#[cfg(test)]
fn fasta(seq: &[u8]) -> SequenceRecord {
    SequenceRecord::Fasta {
        id: "id".to_string(),
        description: None,
        seq: seq.to_vec(),
    }
}

#[test]
fn test_summarise_cumulative() {
    // Patterns separated by non-matching `N` runs so each gap renders as `..`.
    let patterns = vec![
        test_pattern("AAAA"),
        test_pattern("CCCC"),
        test_pattern("GGGG"),
        test_pattern("TTTT"),
    ];

    // "..AAAA..CCCC..GGGG.." (3 matches) is a prefix block of
    // "..AAAA..CCCC..GGGG..TTTT.." (4 matches).
    let short = b"NNAAAANNCCCCNNGGGGNN";
    let long = b"NNAAAANNCCCCNNGGGGNNTTTTNN";
    let one = b"NNAAAANN"; // single match: now also accumulates containing combinations

    let mut reads = Vec::new();
    for _ in 0..3 { reads.push(fasta(short)); }
    for _ in 0..2 { reads.push(fasta(long)); }
    for _ in 0..5 { reads.push(fasta(one)); }

    let rows = summarise_reads(&reads, &patterns, true);

    let render = |parts: &[ReadParts]| -> String {
        parts.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("")
    };
    let find = |needle: &str| -> &SummaryRow {
        rows.iter().find(|r| render(&r.parts) == needle).expect("row present")
    };

    let short_row = find("..AAAA..CCCC..GGGG..");
    assert_eq!(short_row.exact_count, 3);
    assert_eq!(short_row.count, 5, "short should add the 2 long reads containing it");

    let long_row = find("..AAAA..CCCC..GGGG..TTTT..");
    assert_eq!(long_row.exact_count, 2);
    assert_eq!(long_row.count, 2, "long has no superset");

    // The single AAAA match is a contiguous token of both the short (3) and long (2)
    // combinations, so its cumulative count is own 5 + 3 + 2 = 10.
    let one_row = find("..AAAA..");
    assert_eq!(one_row.exact_count, 5);
    assert_eq!(one_row.count, 10, "single-match group also accumulates containing combinations");
}

/// A summarised read category with its counts.
pub struct SummaryRow {
    pub parts: Vec<ReadParts>,
    /// Cumulative count: own count plus the counts of every longer combination
    /// that contains this one (the `number_of_read` column).
    pub count: usize,
    /// Original per-group count, excluding contributions from supersets
    /// (the `exact_count` column).
    pub exact_count: usize,
}

/// Return the sub-slice of `parts` with leading and trailing `Space` removed.
/// `NegativeSpace` only ever appears between matches, so trimming `Space` is
/// sufficient to make read-boundary flanks irrelevant to containment.
fn trim_flanking_space(parts: &[ReadParts]) -> &[ReadParts] {
    let start = parts
        .iter()
        .position(|p| !matches!(p, ReadParts::Space))
        .unwrap_or(parts.len());
    let end = parts
        .iter()
        .rposition(|p| !matches!(p, ReadParts::Space))
        .map_or(start, |i| i + 1);
    &parts[start..end]
}

/// Returns true if the (already trimmed) `shorter` combination appears as a
/// strictly-contiguous sub-slice of the (already trimmed) `longer` one, with
/// separators matching exactly. Requires `shorter` to be strictly shorter so
/// identical combinations do not aggregate into each other.
fn is_contained(shorter: &[ReadParts], longer: &[ReadParts]) -> bool {
    if shorter.is_empty() || shorter.len() >= longer.len() {
        return false;
    }
    longer.windows(shorter.len()).any(|w| w == shorter)
}

/// Catagories reads and reutrn counts for each category
pub fn summarise_reads(
    reads: &[SequenceRecord],
    search_patterns: &[SearchPattern],
    as_counts: bool
) -> Vec<SummaryRow> {
    let mut map: HashMap<Vec<ReadParts>, usize> = HashMap::new();
    for read in reads {
        let read_parts = categorise_read(read, search_patterns);
        let count = map.entry(read_parts).or_insert(0);
        *count += 1;
    }

    // Collect raw (parts, exact_count) groups.
    let groups: Vec<(Vec<ReadParts>, usize)> = map.into_iter().collect();

    // Pre-compute trimmed views once for the O(n^2) containment scan.
    let trimmed: Vec<&[ReadParts]> = groups
        .iter()
        .map(|(parts, _)| trim_flanking_space(parts))
        .collect();

    // For each group, add the counts of every longer group that contains it.
    // Containment is transitive, so summing all containing supersets directly
    // yields the full cumulative total.
    let mut ret: Vec<SummaryRow> = groups
        .iter()
        .enumerate()
        .map(|(i, (parts, exact))| {
            let mut cumulative = *exact;
            for (j, (_, other_exact)) in groups.iter().enumerate() {
                if i != j && is_contained(trimmed[i], trimmed[j]) {
                    cumulative += *other_exact;
                }
            }
            SummaryRow {
                parts: parts.clone(),
                count: cumulative,
                exact_count: *exact,
            }
        })
        .collect();

    // sort by exact (per-group) count and return
    ret.sort_by_key(|row| row.exact_count);
    // into percentage
    if !as_counts {
        let total: f64 = reads.len() as f64 / 100.0;
        ret.iter_mut().for_each(|row| {
            row.count = (row.count as f64 / total).round() as usize;
            row.exact_count = (row.exact_count as f64 / total).round() as usize;
        });
    }
    ret
}

/// format summrised catagories
pub fn fmt_summarised_reads(summarised_reads: &[SummaryRow], as_counts: bool) -> String {
    let mut ret = String::new();
    let suffix = if as_counts { "" } else { "%" };
    for row in summarised_reads {
        ret.push_str(
            format!(
                "{}{}\t{}{}\t{}\n",
                row.count,
                suffix,
                row.exact_count,
                suffix,
                row.parts
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<String>>()
                    .join("")
            )
            .as_str(),
        );
    }
    ret
}
