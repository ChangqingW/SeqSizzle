use std::{io, panic};
use std::str::FromStr;
use std::io::Read;
use bio::bio_types::annot::ParseAnnotError::Splicing;

pub type CrosstermTerminal = ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stderr>>;
use ratatui::{
    prelude::{Constraint, Direction, Frame, Layout, Line, Rect, Color, Modifier},
    widgets::{Block, Borders, Paragraph, Wrap, Clear, List, ListItem, ListState, Widget, StatefulWidget},
};
use ratatui::buffer::Buffer;
use ratatui::style::Style;
use ratatui::text::Span;
use tui_textarea::TextArea;
use crate::{app::{App, UIMode}, event::EventHandler};

pub fn render(view_buffer: Vec<Line>, app: &mut App, frame: &mut Frame) {
    let scroll: u16 = line_num_to_scroll(&view_buffer, app.line_num, frame.size().width - 2);
    frame.render_widget(
        Paragraph::new(view_buffer)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        frame.size(),
    );
    if let UIMode::SearchPanel(_) = app.mode {
            let center_area = centered_rect(80, 80, frame.size());
            let search_panel: SearchPanel = SearchPanel::new(&app);
            frame.render_widget(Clear, center_area);
            //frame.render_widget(Block::default().title("Test popup").borders(Borders::all()), center_area);
            frame.render_stateful_widget(search_panel, center_area, &mut app.mode);
        }
}

/// helper function to create a centered rect using up certain percentage of the available rect `r`
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn line_num_to_scroll(text: &[Line], line_num: usize, row_len: u16) -> u16 {
    if (row_len == 0) | (line_num == 0) {
        0
    } else {
        text[..line_num].iter()
            .map(|x| (x.width() as u16 + row_len - 1) / row_len) // ceiling division
            .sum()
    }
}

fn search_patterns_to_list<'a>(search_patterns: &[(String, String, u8)]) -> List<'a> {
    List::new(search_patterns.iter()
        .map(|(pattern, color, distance)| {
            ListItem::new(Line::from(vec![
                Span::from(pattern.clone()),
                Span::from(", color: "),
                Span::styled(color.clone(), Style::new().fg(Color::from_str(color).unwrap())),
                Span::from(format!(", edit-distance: {}", distance))
            ]))
        })
        .collect::<Vec<ListItem>>())
        .block(Block::default().title("Search patterns").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
        .highlight_symbol(">del ")
}
struct SearchPanel<'a> {
    patterns_list: List<'a>,
    input_pattern: TextArea<'a>,
    input_color: TextArea<'a>,
//    input_button: dyn Widget
}

impl SearchPanel<'_> {
    fn new(app: &App) -> Self {
        Self {
            patterns_list: search_patterns_to_list(&app.search_patterns),
            input_pattern: TextArea::default(),
            input_color: TextArea::default()
        }
    }
}

impl StatefulWidget for SearchPanel<'_> {
    type State = UIMode;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut UIMode) {
        Widget::render(self.patterns_list, area, buf);
    }
}

//let layout = Layout::default()
//.direction(Direction::Vertical)
//.constraints(vec![Constraint::Percentage(80), Constraint::Percentage(20)])
//.split(area);
