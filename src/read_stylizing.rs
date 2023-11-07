use ratatui::prelude::{Line, Span, Style, Stylize};


pub fn merge_intervals(mut intervals: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    // Sort intervals based on the start value
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

pub fn highligh_matches(intervals: Vec<(usize, usize)>, input_string: String) -> Line<'static> {
    let mut result: Vec<Span> = Vec::new();
    let mut current_index = 0;

    for (start, end) in intervals {
        if current_index < start {
            result.push(input_string[current_index..start].to_string().into());
        }
        if end <= input_string.len() {
            result.push(Span::styled(
                input_string[start..end].to_string(),
                Style::new().green(),
            ));
        }
        current_index = end;
    }

    if current_index < input_string.len() {
        result.push(input_string[current_index..].to_string().into());
    }

    Line::from(result)
}
