use ratatui::{
    prelude::{Constraint, Direction, Frame, Layout, Line, Rect, Buffer},
    widgets::{Block, Borders, Clear, ListState, Paragraph, StatefulWidget, Widget, Wrap},
};
use crate::app::{App, UIMode, SearchPanel};
use rayon::prelude::*;

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
        frame.render_widget(Clear, center_area);
        //frame.render_widget(Block::default().title("Test popup").borders(Borders::all()), center_area);
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

pub fn line_num_to_scroll(text: &[Line], line_num: usize, row_len: u16) -> u16 {
    text[..line_num.min(text.len())]
        .par_iter()
        .map(|x| (x.width() as u16 + row_len - 1) / row_len) // ceiling division
        .sum()
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
