pub mod app;
pub mod read_stylizing;
pub mod tui;
mod ui;
pub mod event;


use app::{App, UIMode};
use tui::Tui;
use event::{Event, EventHandler};
use anyhow::Result;
use std::env;
use ratatui::prelude::{CrosstermBackend, Terminal};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn main() -> Result<()> {
    // Initialize App
    let args: Vec<String> = env::args().collect();
    let mut app = App::new(args[1].to_string());

    // Initialize the terminal user interface.
    let backend = CrosstermBackend::new(std::io::stderr());
    let terminal = Terminal::new(backend)?;
    let events = EventHandler::new(250);
    let mut tui = Tui::new(terminal, events);
    tui.enter()?;

    // Start the main loop.
    while !app.quit {
        // Render the user interface.
        tui.draw(&mut app)?;
        // Handle events.
        match app.mode {
            UIMode::Viewer => {
                match tui.events.next()? {
                    Event::Key(KeyEvent {
                                   code: KeyCode::Char('c'),
                                   modifiers: KeyModifiers::CONTROL,
                                   ..
                               })
                    | Event::Key(KeyEvent {
                                     code: KeyCode::Char('q'),
                                     modifiers: KeyModifiers::NONE,
                                     ..
                                 }) => { app.quit() }

                    Event::Key(KeyEvent {
                                   code: KeyCode::Char('j'),
                                   modifiers: KeyModifiers::NONE,
                                   ..
                               }) => { app.line_num += 1 }
                    Event::Key(KeyEvent {
                                   code: KeyCode::Char('k'),
                                   modifiers: KeyModifiers::NONE,
                                   ..
                               }) => {
                        if app.line_num != 0 {
                            app.line_num -= 1;
                        }
                    }

                    Event::Key(KeyEvent {
                                   code: KeyCode::Char('d'),
                                   modifiers: KeyModifiers::CONTROL,
                                   ..
                               }) => { app.line_num += (tui.size().height as f32 * 0.4).floor() as usize }
                    Event::Key(KeyEvent {
                                   code: KeyCode::Char('u'),
                                   modifiers: KeyModifiers::CONTROL,
                                   ..
                               }) => {
                        let up: usize = (tui.size().height as f32 * 0.4).floor() as usize ;
                        app.line_num = if app.line_num < up { 0 } else { app.line_num - up };
                    }

                    Event::Key(KeyEvent {
                                   code: KeyCode::Char('g'),
                                   modifiers: KeyModifiers::NONE,
                                   ..
                               }) => {
                        let mut next: Event = tui.events.next()?;
                        while let Event::Tick = next {
                            next = tui.events.next()?;
                        }
                        if let Event::Key(KeyEvent {
                                              code: KeyCode::Char('g'),
                                              modifiers: KeyModifiers::NONE,
                                              ..
                                          }) = next {
                            app.line_num = 0
                        }
                    },

                    Event::Key(KeyEvent {
                                   code: KeyCode::Char('/'),
                                   modifiers: KeyModifiers::NONE,
                                   ..
                               }) => {
                        app.mode = UIMode::SearchPopup;
                        continue
                    }

                    _ => {}
                }
            }
            UIMode::SearchPopup => {
                match tui.events.next()? {
                    Event::Key(KeyEvent {
                                   code: KeyCode::Char('/'),
                                   modifiers: KeyModifiers::NONE,
                                   ..
                               }) => {
                        app.mode = UIMode::Viewer;
                        continue
                    },
                    _ => {}
                }
            }
        }
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
