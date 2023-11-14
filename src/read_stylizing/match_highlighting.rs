use ratatui::prelude::{Line, Span, Style, Stylize, Color};
use std::str::FromStr;
use gcollections::ops::Bounded;
use interval::IntervalSet;
use interval::ops::Width;
use crate::read_stylizing::interval_operations::find_intersections;

use gcollections::ops::set::{Difference};

fn format_overlap<'a, Bound: Width + num_traits::Num>(
    intervals: &Vec<(IntervalSet<Bound>, &'a str)>,
    overlap_color: &'a str,
) -> Vec<(IntervalSet<Bound>, &'a str)> {
    let overlapped_intervals:IntervalSet<Bound> = find_intersections(&intervals
        .iter()
        .map(|(set, _)| set.clone())
        .collect::<Vec<IntervalSet<Bound>>>());

    let mut result: Vec<(IntervalSet<Bound>, &str)> = Vec::new();
    for (matches, color) in intervals {
        result.push((matches.difference(&overlapped_intervals), *color))
    }
    result.push((overlapped_intervals, overlap_color));

    result
}

pub fn highlight_matches<'a, T: Width + num_traits::PrimInt>(
    intervals: &Vec<(IntervalSet<T>, &str)>,
    input_string: String,
    overlap_color: &str,
) -> Line<'a>
where
T: Into<usize>
    {
    let intervals: Vec<(IntervalSet<T>, &str)> = format_overlap(intervals, overlap_color);
    let mut intervals: Vec<(usize, usize, &str)> = intervals
        .into_iter()
        .flat_map(|(set, color)| {
            set.into_iter()
                .map(move |interval| (interval.lower().into(), interval.upper().into(), color.clone()))
        })
        .collect();
    intervals.sort_by_key(|&(start, _, _)| start);
    let mut result: Vec<Span> = Vec::new();
    let mut current_index:usize = 0;

    for (start, end, color) in intervals.iter().map(|&(a, b, col)| (a, b + 1, col)) {
        if current_index < start {
            result.push(input_string[current_index..start].to_string().into());
        }
        if end <= input_string.len() {
            result.push(Span::styled(
                input_string[start..end ].to_string(),
                Style::new().fg(Color::from_str(color).unwrap()),
            ));
        }
        current_index = end;
    }

    if current_index < input_string.len() {
        result.push(input_string[current_index..].to_string().into());
    }

    Line::from(result)
}
