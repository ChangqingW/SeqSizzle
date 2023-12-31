use crate::app::{App, SearchPanel, UIMode};
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
    if let UIMode::SearchPanel(_) = app.mode {
        let center_area = centered_rect(80, 80, frame.size());
        frame.render_widget(Clear, center_area);
        frame.render_stateful_widget(&app.search_panel, center_area, &mut app.mode);
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

impl StatefulWidget for &SearchPanel<'_> {
    type State = UIMode;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut UIMode) {
        let mut list_state = ListState::default()
            .with_selected(state.get_search_panel_state().patterns_list_selection);
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Percentage(80), Constraint::Percentage(20)])
            .split(area);
        let patterns_list_area: Rect = layout[0];
        let pattern_inputs_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(layout[1]);
        StatefulWidget::render(
            self.patterns_list.clone(),
            patterns_list_area,
            buf,
            &mut list_state,
        );
        self.input_pattern
            .widget()
            .render(pattern_inputs_areas[0], buf);
        self.input_color
            .widget()
            .render(pattern_inputs_areas[1], buf);
        self.input_distance
            .widget()
            .render(pattern_inputs_areas[2], buf);
        self.input_button
            .clone()
            .render(pattern_inputs_areas[3], buf);
    }
}
