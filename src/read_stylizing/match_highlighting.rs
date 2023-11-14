use ratatui::prelude::{Line, Span, Style, Stylize, Color};
use std::str::FromStr;
use interval::ops::*;
use crate::read_stylizing::interval_operations::{merge_intervals, find_intersections, subtract_intervals};

fn format_overlap<'a>(
    intervals: &Vec<(Vec<(usize, usize)>, &'a str)>,
    overlap_color: &'a str,
) -> Vec<(Vec<(usize, usize)>, &'a str)> {
    let intervals: Vec<(Vec<(usize, usize)>, &str)> = intervals
        .iter()
        .map(|(vector, color)| (merge_intervals(vector), color.clone()))
        .collect();
    let overlapped_intervals: Vec<(usize, usize)> = find_intersections(
        &intervals
            .iter()
            .flat_map(|(tuple, _)| tuple.iter().cloned())
            .collect::<Vec<(usize, usize)>>(),
    );

    let mut result: Vec<(Vec<(usize, usize)>, &str)> = Vec::new();
    for (matches, color) in intervals {
        result.push((subtract_intervals(&matches, &overlapped_intervals), color));
    }
    result.push((overlapped_intervals, overlap_color));

    result
}

pub fn highligh_matches<'a>(
    intervals: &Vec<(Vec<(usize, usize)>, &str)>,
    input_string: String,
    overlap_color: &str,
) -> Line<'a> {
    let mut intervals: Vec<(Vec<(usize, usize)>, &str)> = format_overlap(intervals, overlap_color);
    let mut intervals: Vec<(usize, usize, &str)> = intervals
        .iter()
        .flat_map(|(vect, color)| {
            vect.iter()
                .map(move |&(start, end)| (start, end, color.clone()))
        })
        .collect();
    intervals.sort_by_key(|&(start, _, _)| start);
    let mut result: Vec<Span> = Vec::new();
    let mut current_index = 0;

    for (start, end, color) in intervals {
        if current_index < start {
            result.push(input_string[current_index..start].to_string().into());
        }
        if end <= input_string.len() {
            result.push(Span::styled(
                input_string[start..end].to_string(),
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
