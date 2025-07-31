use crate::app::{App, SearchPattern, UIMode};
use crate::search_panel::{PanelElement, PanelElementName};
use crate::{Event, Tui};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::{Color, Size};
use std::str::FromStr;

pub enum Update {
    SearchPanelFocusNext(bool),
    SearchPanelInput(KeyEvent),
    SaveFilePopupInput(KeyEvent),
    ToggleFilePopup,
    EditSearchPattern(SearchPatternEdit),
    CycleSearchPattern(bool),
    ToggleUIMode,
    ScrollViewer(isize),
    WindowResize(Size),
    ToggleQualityItalic,
    ToggleQualityBackground,
    Msg(String),
    Quit,
    None,
}

pub enum SearchPatternEdit {
    Delete(usize, bool), // (index, pop into edit boxes?)
    Append(SearchPattern),
}

pub fn handle_input(app: &App, tui: &Tui, input: Event) -> Update {
    match input {
        Event::Key(KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            ..
        }) => Update::Quit,
        Event::Key(KeyEvent {
            code: KeyCode::Char('/'),
            modifiers: KeyModifiers::NONE,
            ..
        }) => Update::ToggleUIMode,
        Event::Key(KeyEvent { 
            code: KeyCode::Char('f'),
            modifiers: KeyModifiers::CONTROL,
            .. 
        }) => Update::ToggleUIMode,
        Event::Key(keyevent) => match app.mode {
            UIMode::Viewer => handle_input_viewer(app, tui, keyevent),
            UIMode::SearchPanel(false) => handle_input_search_panel(app, tui, keyevent),
            UIMode::SearchPanel(true) => handle_input_file_save(app, tui, keyevent),
        },
        Event::Resize(_, _) => Update::WindowResize(tui.size()),
        _ => Update::None,
    }
}

fn handle_input_file_save(app: &App, tui: &Tui, keyevent: KeyEvent) -> Update {
    if keyevent.code == KeyCode::Esc {
        Update::ToggleFilePopup
    } else if keyevent.code == KeyCode::Enter {
        if let Some(msg) = app.save_patterns() {
            Update::Msg(msg)
        } else {
            Update::ToggleFilePopup
        }
    } else {
        Update::SaveFilePopupInput(keyevent)
    }
}

pub fn handle_input_viewer(app: &App, tui: &Tui, keyevent: KeyEvent) -> Update {
    match keyevent {
        KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            ..
        } => Update::Quit,
        KeyEvent {
            code: KeyCode::Char('j') | KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            ..
        } => Update::ScrollViewer(1),
        KeyEvent {
            code: KeyCode::Char('k') | KeyCode::Up,
            modifiers: KeyModifiers::NONE,
            ..
        } => Update::ScrollViewer(-1),
        KeyEvent {
            code: KeyCode::Char('d'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Update::ScrollViewer((tui.size().height as f32 * 0.4).floor() as isize),
        KeyEvent {
            code: KeyCode::Char('u'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Update::ScrollViewer(-(tui.size().height as f32 * 0.4).floor() as isize),

        // gg scrolls to top
        KeyEvent {
            code: KeyCode::Char('g'),
            modifiers: KeyModifiers::NONE,
            ..
        } => {
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
                Update::ScrollViewer(isize::MIN + 1)// negating isize::MIN cause overflow
            } else {
                Update::None
            }
        },
        
        // Toggle quality italic styling with 'i'
        KeyEvent {
            code: KeyCode::Char('i'),
            modifiers: KeyModifiers::NONE,
            ..
        } => Update::ToggleQualityItalic,
        
        // Toggle quality background styling with 'b'
        KeyEvent {
            code: KeyCode::Char('b'),
            modifiers: KeyModifiers::NONE,
            ..
        } => Update::ToggleQualityBackground,
        _ => Update::None,
    }
}

pub fn handle_input_search_panel(app: &App, tui: &Tui, keyevent: KeyEvent) -> Update {
    if keyevent.code == KeyCode::Esc {
        Update::ToggleUIMode

    // arrow keys switch input focus regardless of current focus
    } else if keyevent.modifiers == KeyModifiers::NONE
        && [KeyCode::Right, KeyCode::Left].contains(&keyevent.code)
    {
        Update::SearchPanelFocusNext(keyevent.code == KeyCode::Left)

    // Tab and S-Tab switch input focus regardless of current focus
    } else if [KeyModifiers::NONE, KeyModifiers::SHIFT].contains(&keyevent.modifiers)
        && [KeyCode::Tab, KeyCode::BackTab].contains(&keyevent.code)
    {
        Update::SearchPanelFocusNext(keyevent.modifiers == KeyModifiers::SHIFT)

    } else if keyevent.modifiers == KeyModifiers::CONTROL && keyevent.code == KeyCode::Char('s') {
        Update::ToggleFilePopup

    // patterns list specific keybindings
    } else if app.search_panel.focused_element() == PanelElementName::PatternsList {
        match keyevent {
            KeyEvent {
                code: KeyCode::Up | KeyCode::Down,
                modifiers: KeyModifiers::NONE,
                ..
            } => Update::CycleSearchPattern(keyevent.code == KeyCode::Up),
            KeyEvent {
                code: KeyCode::Char('d') | KeyCode::Delete | KeyCode::Enter | KeyCode::Backspace,
                modifiers: KeyModifiers::NONE,
                ..
            } => match app.search_panel.selected_pattern() {
                Some(selection) => Update::EditSearchPattern(SearchPatternEdit::Delete(
                    selection,
                    keyevent.code == KeyCode::Enter,
                )),
                None => Update::Msg("No pattern selected".to_string()),
            },
            _ => Update::None,
        }

    // focus != PanelElementName::PatternsList
    } else if keyevent.modifiers == KeyModifiers::NONE && keyevent.code == KeyCode::Enter {
        let search_string = match app.search_panel.elements()[&PanelElementName::InputPattern] {
            PanelElement::TextAreaElement(ref textarea) => textarea.lines().join(""),
            _ => panic!("Wrong type of element"),
        };
        if search_string.is_empty() {
            return Update::Msg("Search pattern cannot be empty".to_string());
        }
        let try_color = Color::from_str(
            match app.search_panel.elements()[&PanelElementName::InputColor] {
                PanelElement::TextAreaElement(ref textarea) => textarea.lines().join(""),
                _ => panic!("Wrong type of element"),
            }
            .as_str(),
        );
        let try_u8 = u8::from_str(
            match app.search_panel.elements()[&PanelElementName::InputDistance] {
                PanelElement::TextAreaElement(ref textarea) => textarea.lines().join(""),
                _ => panic!("Wrong type of element"),
            }
            .as_str(),
        );
        let comment = match app.search_panel.elements()[&PanelElementName::InputComment] {
            PanelElement::TextAreaElement(ref textarea) => textarea.lines().join(""),
            _ => panic!("Wrong type of element"),
        };
        match (try_color, try_u8) {
                                       (Ok(color), Ok(distance)) => {Update::EditSearchPattern(SearchPatternEdit::Append(SearchPattern::new(search_string, color, distance, comment.as_str())))},
                                       (Err(_), Ok(_)) => {Update::Msg("Color needs to be valid hex code".to_string())},
                                       (Ok(_), Err(_)) => {Update::Msg("Edit distance needs to be valid positive integer".to_string())},
                                       (Err(_), Err(_)) => {Update::Msg("Color needs to be valid hex code, edit distance needs to be valid positive integer".to_string())},
                }

    // pass to input boxes
    } else {
        Update::SearchPanelInput(keyevent)
    }
}
