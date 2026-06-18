use crate::app::{App, QualityStyleMode, UIMode};


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
        let area = options_area(frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title("Options")
                .title_top(Line::from("[x]").alignment(Alignment::Right))
                .title_bottom(Line::from("Esc to close").alignment(Alignment::Right)),
            area,
        );
        let italic_on = matches!(
            app.styling_config.quality_style_mode,
            QualityStyleMode::Italic | QualityStyleMode::Both
        );
        let background_on = matches!(
            app.styling_config.quality_style_mode,
            QualityStyleMode::Background | QualityStyleMode::Both
        );
        for (i, item) in OPTION_ITEMS.iter().enumerate() {
            let (on, label, desc) = match item {
                OptionItem::Copy => (
                    app.copy,
                    "Copy mode (c)",
                    "(press c to enable clicking again)",
                ),
                OptionItem::Italic => (italic_on, "Low-quality italic (i)", ""),
                OptionItem::Background => {
                    (background_on, "Quality background coloring (b)", "")
                }
            };
            let check = if on { "[x]" } else { "[ ]" };
            let check_style = Style::default().fg(if on { Color::Green } else { Color::DarkGray });
            let row = option_row(area, i);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!("{check} "), check_style),
                    Span::raw(label),
                ])),
                row,
            );
            if !desc.is_empty() {
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        format!("    {desc}"),
                        Style::default().fg(Color::DarkGray),
                    )),
                    Rect::new(row.x, row.y + 1, row.width, 1),
                );
            }
        }
    }

    if let UIMode::Help = app.mode {
        let area = centered_rect(50, 60, frame.area());
        frame.render_widget(Clear, area);
        let bindings = [
            ("/ or Ctrl-f ", "toggle search panel"),
            ("j / k or \u{2193} / \u{2191} ", "scroll down / up"),
            ("Ctrl-d / Ctrl-u ", "page down / up"),
            ("gg ", "jump to top"),
            ("i ", "toggle low-quality italic"),
            ("b ", "toggle quality background"),
            ("? ", "this help"),
            ("q ", "quit"),
            ("Esc ", "close popup / panel"),
            ("c ", "toggle copy mode"),
        ];
        let mut lines: Vec<Line> = bindings
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
        lines.push(Line::from(Span::styled(
                    "Clicking and mouse text-selection can't both be active: copy mode \
                    releases the mouse so the terminal can select text. Most terminals also \
                    let you hold Shift and drag to select text with copy mode off.",
                    Style::default().fg(Color::DarkGray)
                    )));
                
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

/// Toggles shown in the Options pane, in display order. Single source of truth shared by
/// rendering (ui.rs) and click mapping (control.rs).
#[derive(Clone, Copy)]
pub enum OptionItem {
    Copy,
    Italic,
    Background,
}
pub const OPTION_ITEMS: [OptionItem; 3] =
    [OptionItem::Copy, OptionItem::Italic, OptionItem::Background];

// Options-pane layout, relative to the pane rect's top-left.
const OPT_X_PAD: u16 = 2;
const OPT_Y_PAD: u16 = 2;
const OPT_ROW_STRIDE: u16 = 2; // toggle line + description/spacer line

/// The rect (centered popup) used for the Options pane.
pub fn options_area(area: Rect) -> Rect {
    centered_rect(60, 60, area)
}

/// The toggle line rect for option `index` within the Options pane `area`.
pub fn option_row(area: Rect, index: usize) -> Rect {
    let x = area.x + OPT_X_PAD;
    let y = area.y + OPT_Y_PAD + (index as u16) * OPT_ROW_STRIDE;
    let width = area.width.saturating_sub(2 * OPT_X_PAD);
    Rect::new(x, y, width, 1)
}

/// Which option's clickable band (its toggle line + description/spacer line) contains
/// (col, row), if any.
pub fn options_hit(area: Rect, col: u16, row: u16) -> Option<usize> {
    for i in 0..OPTION_ITEMS.len() {
        let r = option_row(area, i);
        if col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + OPT_ROW_STRIDE {
            return Some(i);
        }
    }
    None
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
