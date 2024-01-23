use crate::app::{App, UIMode};
use crate::search_panel::SearchPanel;
use ratatui::{
    prelude::{Buffer, Color, Constraint, Direction, Frame, Layout, Line, Rect, Span, Style},
    widgets::{
        block::title::{Position, Title},
        Block, Borders, Clear, ListState, Paragraph, StatefulWidget, Widget, Wrap,
    },
};

pub fn render(app: &mut App, frame: &mut Frame) {
    let viewer_block = match app.get_message() {
        Some(msg) => Block::default()
                .borders(Borders::ALL)
                .title(app.file.to_str().unwrap_or("SeqSizzle"))
                .title(Title::from(Span::styled(msg, Style::default().fg(Color::Red)))
                .position(Position::Bottom))
            ,
        None => Block::default()
                .borders(Borders::ALL)
                .title(app.file.to_str().unwrap_or("SeqSizzle"))
    };

    frame.render_widget(
        Paragraph::new(
            app.rendered_lines
                .clone()
                .into_iter()
                .collect::<Vec<Line>>(),
        )
        .block(viewer_block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_status.1 as u16, 0)),
        frame.size(),
    );
    if let UIMode::SearchPanel = app.mode {
        let center_area = centered_rect(80, 80, frame.size());
        frame.render_widget(Clear, center_area);
        frame.render_widget(&app.search_panel, center_area);
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
