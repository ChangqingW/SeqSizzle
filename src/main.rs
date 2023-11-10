pub mod app;
pub mod read_stylizing;
pub mod tui;
pub mod event;

use app::App;
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
        match tui.events.next()? {
            Event::Key(key_event) => {
                match key_event.code {
                    KeyCode::Char('q') => {break},
                    KeyCode::Char('j') => {tui.scroll_idx += 1},
                    KeyCode::Char('k') => {
                        if tui.scroll_idx != 0 {
                            tui.scroll_idx -= 1;
                        }
                    },
                    _ => {}
                }
            },
            _ => {}
        };
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
