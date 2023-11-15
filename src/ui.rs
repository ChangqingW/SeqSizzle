use std::{io, panic};
pub type CrosstermTerminal = ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stderr>>;
use ratatui::{
    prelude::{Constraint, Direction, Frame, Layout, Line, Rect},
    widgets::{Block, Borders, Paragraph, Wrap, Clear},
};
use tui_textarea::TextArea;
use crate::{app::{App, UIMode}, event::EventHandler};

pub fn render(view_buffer: Vec<Line>, app: &App, frame: &mut Frame) {
    frame.render_widget(
        Paragraph::new(view_buffer)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((app.line_num, 0)),
        frame.size(),
    );
    match app.mode {
        UIMode::SearchPopup => {
            let center_area = centered_rect(80, 80, frame.size());
            frame.render_widget(Clear, center_area);
            frame.render_widget(Block::default().title("Test popup").borders(Borders::all()), center_area);
        }
        _ => {return}
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