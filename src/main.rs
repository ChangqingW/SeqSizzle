pub mod app;
pub mod read_stylizing;
pub mod tui;
mod ui;
pub mod control;
pub mod event;


use app::{App, UIMode};
use tui::Tui;
use event::{Event, EventHandler};
use anyhow::Result;
use std::env;
use ratatui::prelude::{CrosstermBackend, Terminal};
use crate::control::{handle_input, Update};

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
        let updates: Update = handle_input(&app, &tui, tui.events.next()?);
        match updates {
            Update::None => {},
            Update::EditSearchPattern(_) => {},
            Update::ToggleUIMode => app.toggle_ui_mode(),
            Update::ScrollViewer(num) => {app.scroll(num);},
            Update::Quit => app.quit = true
        }
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
