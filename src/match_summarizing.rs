use crate::app::{App, SearchPattern};
use bio::io::fastq;
use ratatui::style::Color;
use std::collections::HashMap;
use std::collections::VecDeque;

#[derive(PartialEq, Eq, Clone, Hash)]
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
fn categorise_read(record: &fastq::Record, search_patterns: &[SearchPattern]) -> Vec<ReadParts> {
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
    let fastq: fastq::Record = fastq::Record::with_attrs(
        "id",
        None,
        b"ATCGCCATCGCCATCGCCATCGATCAAATCGGATC",
        b"!!!!!!!!!!!!!!!!!!!!!!",
    );
    let patterns = vec![
        SearchPattern::new(String::from("ATCG"), Color::Red, 0, ""),
        SearchPattern::new(String::from("GATC"), Color::Red, 0, ""),
    ];
    let mut result = String::new();
    for i in categorise_read(&fastq, &patterns) {
        match i {
            ReadParts::Match(x) => result.push_str(x.search_string.as_str()),
            ReadParts::Space => result.push_str(".."),
            ReadParts::NegativeSpace => result.push_str("-"),
        }
    }
    assert_eq!(
        result,
        String::from("ATCG..ATCG..ATCG..ATCG-GATC..ATCGGATC")
    );
}

/// Catagories reads and reutrn counts for each category
pub fn summarise_reads(
    reads: &[fastq::Record],
    search_patterns: &[SearchPattern],
) -> Vec<(Vec<ReadParts>, usize)> {
    let mut map: HashMap<Vec<ReadParts>, usize> = HashMap::new();
    for read in reads {
        let read_parts = categorise_read(read, search_patterns);
        let count = map.entry(read_parts).or_insert(0);
        *count += 1;
    }

    // sort by count and return
    let mut ret: Vec<(Vec<ReadParts>, usize)> = map.into_iter().collect();
    ret.sort_by_key(|x| x.1);
    // into percentage
    let total: usize = reads.len() / 100;
    ret.iter_mut().for_each(|(_, count)| *count /= total);
    ret
}

/// format summrised catagories
pub fn fmt_summarised_reads(summarised_reads: &[(Vec<ReadParts>, usize)]) -> String {
    let mut ret = String::new();
    for (read_parts, count) in summarised_reads {
        ret.push_str(
            format!(
                "{}%\t{}\n",
                count,
                read_parts
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
