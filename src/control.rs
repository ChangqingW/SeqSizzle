use crate::app::{App, SearchPanelFocus, SearchPattern, UIMode};
use crate::{Event, Tui};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::Color;
use std::str::FromStr;

pub enum Update {
    SearchPanelFocus(SearchPanelFocus),
    SearchPanelInput(SearchPanelFocus, KeyEvent),
    EditSearchPattern(SearchPatternEdit),
    CycleSearchPattern(bool),
    ToggleUIMode,
    ScrollViewer(isize),
    Msg(String),
    Quit,
    None,
}

pub enum SearchPatternEdit {
    Delete(usize, bool), // (index, pop into edit boxes?)
    Append(SearchPattern),
}

fn cycle_focus(focus: SearchPanelFocus, reverse: bool) -> SearchPanelFocus {
    let list = vec![
        SearchPanelFocus::PatternsList,
        SearchPanelFocus::InputPattern,
        SearchPanelFocus::InputColor,
        SearchPanelFocus::InputDistance,
    ];
    let mut index = list.iter().position(|&x| x == focus).unwrap();
    if reverse {
        index = index.checked_sub(1).unwrap_or(list.len() - 1);
    } else {
        index = index.checked_add(1).unwrap_or(0);
    }
    list[if index >= list.len() { 0 } else { index }]
}

pub fn handle_input(app: &App, tui: &Tui, input: Event) -> Update {
    // Quit independent of mode
    match input {
        Event::Key(KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            ..
        })
        | Event::Key(KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            ..
        }) => return Update::Quit,
        Event::Key(KeyEvent {
            code: KeyCode::Char('/'),
            modifiers: KeyModifiers::NONE,
            ..
        }) => return Update::ToggleUIMode,
        _ => {}
    };

    match &app.mode {
        UIMode::Viewer(_) => match input {
            Event::Key(KeyEvent {
                code: KeyCode::Char('j') | KeyCode::Down,
                modifiers: KeyModifiers::NONE,
                ..
            }) => Update::ScrollViewer(1),
            Event::Key(KeyEvent {
                code: KeyCode::Char('k') | KeyCode::Up,
                modifiers: KeyModifiers::NONE,
                ..
            }) => Update::ScrollViewer(-1),
            Event::Key(KeyEvent {
                code: KeyCode::Char('d'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }) => Update::ScrollViewer((tui.size().height as f32 * 0.4).floor() as isize),
            Event::Key(KeyEvent {
                code: KeyCode::Char('u'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }) => Update::ScrollViewer(-(tui.size().height as f32 * 0.4).floor() as isize),
            Event::Key(KeyEvent {
                code: KeyCode::Char('g'),
                modifiers: KeyModifiers::NONE,
                ..
            }) => {
                let mut next: Event = tui.events.next().unwrap();
                while let Event::Tick = next {
                    next = tui.events.next().unwrap();
                }
                if let Event::Key(KeyEvent {
                    code: KeyCode::Char('g'),
                    modifiers: KeyModifiers::NONE,
                    ..
                }) = next
                {
                    return Update::ScrollViewer(isize::MIN + 1); // negating isize::MIN cause overflow
                } else {
                    Update::None
                }
            }

            _ => Update::None,
        },

        UIMode::SearchPanel(state) => match input {
            Event::Key(keyevent) => {
                if keyevent.modifiers == KeyModifiers::ALT {
                    match keyevent.code {
                        KeyCode::Char('1') => {
                            Update::SearchPanelFocus(SearchPanelFocus::PatternsList)
                        }
                        KeyCode::Char('2') => {
                            Update::SearchPanelFocus(SearchPanelFocus::InputPattern)
                        }
                        KeyCode::Char('3') => {
                            Update::SearchPanelFocus(SearchPanelFocus::InputColor)
                        }
                        KeyCode::Char('4') => {
                            Update::SearchPanelFocus(SearchPanelFocus::InputDistance)
                        }
                        KeyCode::Char('5') => {
                            let search_string: String =
                                app.search_panel.input_pattern.lines().join("");
                            let try_color =
                                Color::from_str(&app.search_panel.input_color.lines().join(""));
                            let try_u8 =
                                u8::from_str(&app.search_panel.input_distance.lines().join(""));
                            match (try_color, try_u8) {
                                       (Ok(color), Ok(distance)) => {Update::EditSearchPattern(SearchPatternEdit::Append(SearchPattern::new(search_string, color, distance)))},
                                       (Err(_), Ok(_)) => {Update::Msg("Color needs to be valid hex code".to_string())},
                                       (Ok(_), Err(_)) => {Update::Msg("Edit distance needs to be valid positive integer".to_string())},
                                       (Err(_), Err(_)) => {Update::Msg("Color needs to be valid hex code, edit distance needs to be valid positive integer".to_string())},
                                   }
                        }
                        _ => Update::None,
                    }
                } else if keyevent.modifiers == KeyModifiers::NONE
                    && vec![KeyCode::Right, KeyCode::Left].contains(&keyevent.code)
                {
                    Update::SearchPanelFocus(cycle_focus(
                        state.focus,
                        keyevent.code == KeyCode::Left,
                    ))
                } else if state.focus == SearchPanelFocus::PatternsList {
                    if keyevent.modifiers == KeyModifiers::NONE
                        && vec![KeyCode::Up, KeyCode::Down].contains(&keyevent.code)
                    {
                        Update::CycleSearchPattern(keyevent.code == KeyCode::Up)
                    } else if keyevent.modifiers == KeyModifiers::NONE
                        && vec![KeyCode::Char('d'), KeyCode::Delete].contains(&keyevent.code)
                    {
                        match state.patterns_list_selection {
                            Some(selection) => Update::EditSearchPattern(
                                SearchPatternEdit::Delete(selection, false),
                            ),
                            None => Update::Msg("No pattern selected".to_string()),
                        }
                    } else if keyevent.modifiers == KeyModifiers::NONE
                        && keyevent.code == KeyCode::Enter
                    {
                        match state.patterns_list_selection {
                            Some(selection) => Update::EditSearchPattern(
                                SearchPatternEdit::Delete(selection, true),
                            ),
                            None => Update::Msg("No pattern selected".to_string()),
                        }
                    } else {
                        Update::None
                    }
                } else {
                    Update::SearchPanelInput(state.focus, keyevent)
                }
            }
            _ => Update::None,
        },
    }
}
