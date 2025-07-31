use ratatui::prelude::{Color, Line, Span, Style, Stylize};
use interval::{IntervalSet, interval_set::ToIntervalSet};
use gcollections::ops::Bounded;
use crate::read_stylizing::match_highlighting::format_overlap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombinedStyle {
    pub fg_color: Option<Color>,
    pub bg_color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
}

impl Default for CombinedStyle {
    fn default() -> Self {
        Self {
            fg_color: None,
            bg_color: None,
            bold: false,
            italic: false,
        }
    }
}

impl CombinedStyle {
    pub fn to_ratatui_style(self) -> Style {
        let mut style = Style::default();
        
        if let Some(fg) = self.fg_color {
            style = style.fg(fg);
        }
        if let Some(bg) = self.bg_color {
            style = style.bg(bg);
        }
        if self.bold {
            style = style.bold();
        }
        if self.italic {
            style = style.italic();
        }
        
        style
    }
    
    pub fn with_fg_color(mut self, color: Color) -> Self {
        self.fg_color = Some(color);
        self
    }
    
    pub fn with_bg_color(mut self, color: Color) -> Self {
        self.bg_color = Some(color);
        self
    }
    
    pub fn with_bold(mut self) -> Self {
        self.bold = true;
        self
    }
    
    pub fn with_italic(mut self) -> Self {
        self.italic = true;
        self
    }
}

pub struct StyleInput {
    pub fg_color_intervals: Vec<(IntervalSet<usize>, Color)>,
    pub bg_color_intervals: Vec<(IntervalSet<usize>, Color)>,
    pub bold_positions: Vec<bool>,
    pub italic_positions: Vec<bool>,
}

impl StyleInput {
    pub fn new(sequence_length: usize) -> Self {
        Self {
            fg_color_intervals: Vec::new(),
            bg_color_intervals: Vec::new(),
            bold_positions: vec![false; sequence_length],
            italic_positions: vec![false; sequence_length],
        }
    }
    
    pub fn add_fg_color(&mut self, intervals: IntervalSet<usize>, color: Color) {
        self.fg_color_intervals.push((intervals, color));
    }
    
    pub fn add_bg_color(&mut self, intervals: IntervalSet<usize>, color: Color) {
        self.bg_color_intervals.push((intervals, color));
    }
    
    pub fn set_bold_positions(&mut self, bold_vec: Vec<bool>) {
        self.bold_positions = bold_vec;
    }
    
    pub fn set_italic_positions(&mut self, italic_vec: Vec<bool>) {
        self.italic_positions = italic_vec;
    }
}

/// Convert boolean vector to IntervalSet
pub fn bool_vector_to_intervals(bool_vec: &[bool]) -> IntervalSet<usize> {
    
    let mut intervals = Vec::new();
    let mut start = None;
    
    for (i, &is_active) in bool_vec.iter().enumerate() {
        match (start, is_active) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                if s <= i - 1 {
                    intervals.push((s, i - 1));
                }
                start = None;
            }
            _ => {}
        }
    }
    
    // Handle case where sequence ends with true
    if let Some(s) = start {
        intervals.push((s, bool_vec.len() - 1));
    }
    
    intervals.to_interval_set()
}

/// Quality score to background color mapping
pub fn quality_to_bg_color(quality_score: u8) -> Color {
    match quality_score {
        0..=10 => Color::Red,      // Very low quality
        11..=20 => Color::Yellow,  // Low quality  
        21..=30 => Color::Cyan,    // Medium quality
        _ => Color::Green,         // High quality
    }
}

/// Main function to highlight text with combined styles
pub fn highlight_with_combined_styles<'a>(
    text: String,
    style_input: StyleInput,
    overlap_color: Color,
) -> Line<'a> {
    let text_len = text.len();
    let mut position_styles: Vec<CombinedStyle> = vec![CombinedStyle::default(); text_len];
    
    // 1. Apply foreground colors from intervals (with overlap handling)
    let fg_intervals_with_overlap = format_overlap(&style_input.fg_color_intervals, overlap_color);
    for (intervals, color) in fg_intervals_with_overlap {
        for interval in intervals.iter() {
            let start: usize = interval.lower();
            let end: usize = interval.upper();
            for pos in start..=end.min(text_len - 1) {
                position_styles[pos].fg_color = Some(color);
            }
        }
    }
    
    // 2. Apply background colors from intervals
    for (intervals, color) in &style_input.bg_color_intervals {
        for interval in intervals.iter() {
            let start: usize = interval.lower();
            let end: usize = interval.upper();
            for pos in start..=end.min(text_len - 1) {
                position_styles[pos].bg_color = Some(*color);
            }
        }
    }
    
    // 3. Apply bold from boolean vector
    for (pos, &is_bold) in style_input.bold_positions.iter().enumerate() {
        if pos < text_len && is_bold {
            position_styles[pos].bold = true;
        }
    }
    
    // 4. Apply italic from boolean vector
    for (pos, &is_italic) in style_input.italic_positions.iter().enumerate() {
        if pos < text_len && is_italic {
            position_styles[pos].italic = true;
        }
    }
    
    // 5. Group consecutive positions with same style
    if position_styles.is_empty() {
        return Line::from(text);
    }
    
    let mut spans = Vec::new();
    let mut current_start = 0;
    let mut current_style = position_styles[0].clone();
    
    for (pos, style) in position_styles.iter().enumerate().skip(1) {
        if *style != current_style {
            let span_text = text[current_start..pos].to_string();
            spans.push(create_styled_span(span_text, current_style.clone()));
            current_start = pos;
            current_style = style.clone();
        }
    }
    
    // Add final span
    let final_text = text[current_start..].to_string();
    spans.push(create_styled_span(final_text, current_style));
    
    Line::from(spans)
}

fn create_styled_span(text: String, combined_style: CombinedStyle) -> Span<'static> {
    let ratatui_style = combined_style.to_ratatui_style();
    
    if ratatui_style == Style::default() {
        text.into()
    } else {
        Span::styled(text, ratatui_style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interval::interval_set::ToIntervalSet;
    use gcollections::ops::IsEmpty;

    #[test]
    fn test_bool_vector_to_intervals_empty() {
        let bool_vec = vec![];
        let intervals = bool_vector_to_intervals(&bool_vec);
        assert!(intervals.is_empty());
    }

    #[test]
    fn test_bool_vector_to_intervals_all_false() {
        let bool_vec = vec![false, false, false];
        let intervals = bool_vector_to_intervals(&bool_vec);
        assert!(intervals.is_empty());
    }

    #[test]
    fn test_bool_vector_to_intervals_all_true() {
        let bool_vec = vec![true, true, true];
        let intervals = bool_vector_to_intervals(&bool_vec);
        let expected = vec![(0, 2)].to_interval_set();
        assert_eq!(intervals, expected);
    }

    #[test]
    fn test_bool_vector_to_intervals_mixed() {
        let bool_vec = vec![true, false, true, true, false];
        let intervals = bool_vector_to_intervals(&bool_vec);
        let expected = vec![(0, 0), (2, 3)].to_interval_set();
        assert_eq!(intervals, expected);
    }

    #[test]
    fn test_combined_style_default() {
        let style = CombinedStyle::default();
        assert_eq!(style.fg_color, None);
        assert_eq!(style.bg_color, None);
        assert!(!style.bold);
        assert!(!style.italic);
    }

    #[test]
    fn test_combined_style_chaining() {
        let style = CombinedStyle::default()
            .with_fg_color(Color::Red)
            .with_bg_color(Color::Yellow)
            .with_bold()
            .with_italic();
        
        assert_eq!(style.fg_color, Some(Color::Red));
        assert_eq!(style.bg_color, Some(Color::Yellow));
        assert!(style.bold);
        assert!(style.italic);
    }

    #[test]
    fn test_quality_to_bg_color() {
        assert_eq!(quality_to_bg_color(5), Color::Red);
        assert_eq!(quality_to_bg_color(15), Color::Yellow);
        assert_eq!(quality_to_bg_color(25), Color::Cyan);
        assert_eq!(quality_to_bg_color(35), Color::Green);
    }

    #[test]
    fn test_style_input_new() {
        let style_input = StyleInput::new(5);
        assert_eq!(style_input.fg_color_intervals.len(), 0);
        assert_eq!(style_input.bg_color_intervals.len(), 0);
        assert_eq!(style_input.bold_positions, vec![false; 5]);
        assert_eq!(style_input.italic_positions, vec![false; 5]);
    }
}