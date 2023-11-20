pub mod app;
pub mod read_stylizing;
pub mod tui;
mod ui;
pub mod control;
pub mod event;


use app::{App};
use tui::Tui;
use event::{Event, EventHandler};
use anyhow::Result;
use std::env;
use ratatui::prelude::{CrosstermBackend, Terminal};
use crate::control::{handle_input, SearchPatternEdit, Update};

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
            Update::ToggleUIMode => app.toggle_ui_mode(),
            Update::ScrollViewer(num) => {app.scroll(num);},
            Update::Quit => app.quit = true,
            Update::SearchPanelFocus(focus) => {
                println!("Focus changed to: {:?}", focus);
                app.focus_search_panel(focus);
            },
            Update::SearchPanelInput(focus, input) => {
                match focus {
                    app::SearchPanelFocus::InputPattern => {
                        app.search_panel.input_pattern.input(input);
                        println!("Input to pattern: {:?}", input);
                    },
                    app::SearchPanelFocus::InputColor => _ =app.search_panel.input_color.input(input),
                    app::SearchPanelFocus::InputDistance => _ =app.search_panel.input_distance.input(input),
                    _ => {}
                };
            },
            Update::EditSearchPattern(edit) => {
                match edit {
                    SearchPatternEdit::Append(x) => app.append_search_pattern(x),
                    _ => () // TODO: implement pattern deletion
                }
            },
            Update::Msg(msg) => app.message(msg)
        };
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
