use crate::app::{App, SearchPattern, UIMode};
use crossterm::event::{KeyEvent, KeyModifiers, KeyCode};
use ratatui::prelude::{Rect};
use crate::{Tui, Event};

pub enum Update {
    None,
    EditSearchPattern(SearchPatternEdit),
    ToggleUIMode,
    ScrollViewer(isize),
    Quit,
}

pub enum SearchPatternEdit {
    Delete(usize),
    Append(SearchPattern),
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

    match app.mode {
        UIMode::Viewer => match input {
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
                                  }) = next {
                    return Update::ScrollViewer(isize::MIN + 1) // negating isize::MIN cause overflow
                } else {
                    Update::None
                }
            },

            _ => Update::None,
        },
        UIMode::SearchPanel(_) => Update::None
    }
}
