use ratatui::{
    prelude::{Frame},
    widgets::{Block, Borders, Paragraph, Wrap},
};


use crate::app::App;

pub fn render(app: &mut App, frame: &mut Frame) {
    frame.render_widget(
        Paragraph::new(app.update())
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        frame.size()
    );
}
