use interval::interval_set::*;
use interval::ops::*;
use gcollections::ops::set::*;
use gcollections::ops::Bounded;

pub fn find_intersections(intervals: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut intersections: Vec<(usize, usize)> = Vec::new();

    for i in 0..intervals.len() {
        for j in i + 1..intervals.len() {
            let (start1, end1) = intervals[i];
            let (start2, end2) = intervals[j];

            // Check for overlap
            if end1 >= start2 && end2 >= start1 {
                // Calculate intersection
                let intersection_start = std::cmp::max(start1, start2);
                let intersection_end = std::cmp::min(end1, end2);
                intersections.push((intersection_start, intersection_end));
            }
        }
    }

    merge_intervals(&intersections)
}

#[test]
fn test_find_intersections() {
    let x: Vec<(usize, usize)> = vec![(1,3), (2,4), (5,8), (9,10), (9,10), (9,10)];
    assert_eq!(find_intersections(&x), vec![(2,3), (9,10)]);
}

pub fn merge_intervals(intervals: &[(usize, usize)]) -> Vec<(usize, usize)> {
    // Sort intervals based on the start value
    let mut intervals = intervals.to_vec();
    intervals.sort_by(|a, b| a.0.cmp(&b.0));
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for interval in intervals {
        if let Some(last_merged) = merged.last_mut() {
            // If the current interval overlaps with the last merged interval, merge them
            if interval.0 <= last_merged.1 {
                last_merged.1 = interval.1.max(last_merged.1);
            } else {
                // If no overlap, add the current interval to the merged list
                merged.push(interval);
            }
        } else {
            // If merged list is empty, add the first interval
            merged.push(interval);
        }
    }
    merged
}

#[test]
fn test_merge_intervals() {
    let x: Vec<(usize, usize)> = vec![(1,3), (2,4), (5,8), (9,10), (9,10), (9,10)];
    assert_eq!(merge_intervals(&x), vec![(1,4), (5, 8), (9,10)]);
}


fn find_overlap(
    intervals1: &[(usize, usize)],
    intervals2: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    let mut intervals1 = intervals1.to_vec();
    let mut intervals2 = intervals2.to_vec();
    intervals1.sort_by_key(|interval| interval.0);
    intervals2.sort_by_key(|interval| interval.0);

    let mut overlaps = Vec::new();
    let (mut i, mut j) = (0, 0);

    while i < intervals1.len() && j < intervals2.len() {
        let (start1, end1) = intervals1[i];
        let (start2, end2) = intervals2[j];

        if end1 >= start2 && end2 >= start1 {
            let overlap_start = start1.max(start2);
            let overlap_end = end1.min(end2);
            overlaps.push((overlap_start, overlap_end));
        }

        if end1 < end2 {
            i += 1;
        } else {
            j += 1;
        }
    }

    overlaps
}

pub fn subtract_intervals(
    first_intervals: &[(usize, usize)],
    second_intervals: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    let diff = first_intervals
        .to_vec()
        .to_interval_set()
        .difference(&second_intervals.to_vec().to_interval_set());
    let mut result: Vec<(usize, usize)> = Vec::new();
    for interval in diff {
        result.push((interval.lower(), interval.upper()));
    }
    result
}
#[test]
fn test_subtract_intervals() {
    let x: Vec<(usize, usize)> = vec![(1,3), (4, 8), (9,10)];
    let y: Vec<(usize, usize)> = vec![(2,4), (7,12)];
    assert_eq!(subtract_intervals(&x, &y), vec![(1,1), (5,6)]);
}