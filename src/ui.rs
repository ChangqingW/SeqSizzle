use crate::app::{App, UIMode};


use ratatui::{
    prelude::{Alignment, Color, Constraint, Direction, Frame, Layout, Line, Modifier, Rect, Span, Style},
    widgets::{
        Block, Borders, Clear, Paragraph, Wrap,
    },
};

pub const BTN_OPTIONS: &str = "[options]";
pub const BTN_PATTERNS: &str = "[patterns]";

pub fn render(app: &mut App, frame: &mut Frame) {

    let viewer_block = match app.get_message() {
        Some(msg) => Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .title(app.file.to_str().unwrap_or("SeqSizzle"))
            .title_bottom(
                Line::from(Span::styled(msg, Style::default().fg(Color::Red)))
            ),
        None => Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .title(app.file.to_str().unwrap_or("SeqSizzle")),
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
        frame.area(),
    );
    // bottom-right controls: clickable buttons when interactive, a help hint in copy mode
    if app.copy {
        let hint = "? for help";
        let row = frame.area().y + frame.area().height.saturating_sub(1);
        let w = hint.len() as u16;
        let x = (frame.area().x + frame.area().width).saturating_sub(w + 1);
        frame.render_widget(
            Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
            Rect::new(x, row, w, 1),
        );
    } else {
        let (options, patterns) = viewer_buttons(frame.area());
        let btn_style = Style::default().fg(Color::Cyan);
        frame.render_widget(
            Paragraph::new(Span::styled(BTN_OPTIONS, btn_style)),
            options,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(BTN_PATTERNS, btn_style)),
            patterns,
        );
    }

    if let UIMode::SearchPanel(save_file_popup) = app.mode {
        let center_area = centered_rect(80, 80, frame.area());
        frame.render_widget(Clear, center_area);
        frame.render_widget(&app.search_panel, center_area);
        if save_file_popup {
            let save_file_popup_area = centered_rect(80, 5, frame.area());
            frame.render_widget(Clear, save_file_popup_area);
            frame.render_widget(app.search_panel.file_save_popup_widget() , save_file_popup_area);
        }
    }

    if let UIMode::Options = app.mode {
        let area = centered_rect(60, 60, frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new("Coming soon.")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Options")
                        .title_top(Line::from("[x]").alignment(Alignment::Right))
                        .title_bottom(Line::from("Esc to close").alignment(Alignment::Right)),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    if let UIMode::Help = app.mode {
        let area = centered_rect(50, 60, frame.area());
        frame.render_widget(Clear, area);
        let bindings = [
            ("/  or  Ctrl-f", "toggle search panel"),
            ("c", "toggle copy mode"),
            ("j / k  or  \u{2193} / \u{2191}", "scroll down / up"),
            ("Ctrl-d / Ctrl-u", "page down / up"),
            ("gg", "jump to top"),
            ("i", "toggle low-quality italic"),
            ("b", "toggle quality background"),
            ("?", "this help"),
            ("q", "quit"),
            ("Esc", "close popup / panel"),
        ];
        let lines: Vec<Line> = bindings
            .iter()
            .map(|(key, desc)| {
                Line::from(vec![
                    Span::styled(
                        format!("  {key:<16}"),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(*desc),
                ])
            })
            .collect();
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Key reference")
                        .title_bottom(
                            Line::from("any key to close").alignment(Alignment::Right),
                        ),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }
}

/// Returns the (options, patterns) button rects on the bottom border row of `area`.
/// Shared by rendering and click hit-testing so they stay in sync.
pub fn viewer_buttons(area: Rect) -> (Rect, Rect) {
    let row = area.y + area.height.saturating_sub(1);
    let ow = BTN_OPTIONS.len() as u16;
    let pw = BTN_PATTERNS.len() as u16;
    let right = area.x + area.width;
    // leave 1 col for the bottom-right border corner
    let patterns_x = right.saturating_sub(1 + pw);
    let options_x = patterns_x.saturating_sub(1 + ow); // 1-col gap
    (
        Rect::new(options_x, row, ow, 1),
        Rect::new(patterns_x, row, pw, 1),
    )
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
