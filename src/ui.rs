use ratatui::{
    prelude::{Constraint, Direction, Frame, Layout},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use tui_textarea::TextArea;

use crate::app::App;

pub fn render(app: &mut App, frame: &mut Frame) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Percentage(80), Constraint::Percentage(20)])
        .split(frame.size());

    frame.render_widget(
        Paragraph::new(app.update())
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        layout[0],
    );

    let mut textarea = TextArea::default();
    frame.render_widget(textarea.widget(), layout[1]);
}
