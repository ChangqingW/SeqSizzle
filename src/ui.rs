use crate::{
    app::{App, SearchPanel, UIMode},
    buffer::Read,
};
use ratatui::{
    prelude::{Buffer, Constraint, Direction, Frame, Layout, Line, Rect},
    widgets::{Block, Borders, Clear, ListState, Paragraph, StatefulWidget, Widget, Wrap},
};
use rayon::prelude::*;

pub fn render(app: &mut App, frame: &mut Frame) {
    // let scroll: u16 = line_num_to_scroll(&view_buffer, app.line_num, frame.size().width - 2);
    let size = frame.size();

    // get scroll status
    let (mut read, line) = app.scroll_status;
    if let Some(current_read) = app.records.get_index(read) {
        let mut current_height = current_read.calculate_height(size.width) - line;
        // we subtract a further 1 to avoid cases where there will be a gap
        if current_height != 0 {
            current_height -= 1;
        }

        // first, let's render the topmost read
        frame.render_widget(
            current_read
                .paragraph
                .clone()
                .scroll((line.try_into().unwrap(), 0)),
            frame.size(),
        );

        // TODO: implement this as a ReadBufferIterator
        read += 1;
        loop {
            if let Some(r) = app.records.get_index(read) {
                if current_height >= size.height {
                    break;
                }

                let old_size = size.clone();
                let mut new_size = old_size.clone();
                new_size.y = current_height + 1;
                new_size.height -= new_size.y;
                frame.render_widget(r.paragraph.clone().scroll((0, 0)), new_size);

                let height = r.clone().calculate_height(size.width.into());
                current_height += height;

                read += 1;
            } else {
                break;
            }
        }
    }

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
