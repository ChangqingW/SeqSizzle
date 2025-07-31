use ratatui::prelude::{Color, Line, Span, Style};

use crate::read_stylizing::interval_operations::find_intersections;
use gcollections::ops::Bounded;
use interval::ops::Width;
use interval::IntervalSet;

use gcollections::ops::set::Difference;

pub fn format_overlap<Bound: Width + num_traits::Num, Meta: Copy>(
    intervals: &Vec<(IntervalSet<Bound>, Meta)>,
    overlap_color: Meta,
) -> Vec<(IntervalSet<Bound>, Meta)> {
    let overlapped_intervals: IntervalSet<Bound> = find_intersections(
        &intervals
            .iter()
            .map(|(set, _)| set.clone())
            .collect::<Vec<IntervalSet<Bound>>>(),
    );

    let mut result: Vec<(IntervalSet<Bound>, Meta)> = Vec::new();
    for (matches, color) in intervals {
        result.push((matches.difference(&overlapped_intervals), *color))
    }
    result.push((overlapped_intervals, overlap_color));

    result
}

pub fn highlight_matches<'a, T: Width + num_traits::PrimInt>(
    intervals: &Vec<(IntervalSet<T>, Color)>,
    input_string: String,
    overlap_color: Color,
) -> Line<'a>
where
    T: Into<usize>,
{
    let intervals: Vec<(IntervalSet<T>, Color)> = format_overlap(intervals, overlap_color);
    let mut intervals: Vec<(usize, usize, Color)> = intervals
        .into_iter()
        .flat_map(|(set, color)| {
            set.into_iter().map(move |interval| {
                (
                    interval.lower().into(),
                    interval.upper().into(),
                    color,
                )
            })
        })
        .collect();
    intervals.sort_by_key(|&(start, _, _)| start);
    let mut result: Vec<Span> = Vec::new();
    let mut current_index: usize = 0;

    for (start, end, color) in intervals.iter().map(|&(a, b, col)| (a, b + 1, col)) {
        if current_index < start {
            result.push(input_string[current_index..start].to_string().into());
        }
        if end <= input_string.len() {
            result.push(Span::styled(
                input_string[start..end].to_string(),
                Style::new().fg(color),
            ));
        }
        current_index = end;
    }

    if current_index < input_string.len() {
        result.push(input_string[current_index..].to_string().into());
    }

    Line::from(result)
}
