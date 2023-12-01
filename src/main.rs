pub mod app;
pub mod buffer;
pub mod control;
pub mod event;
pub mod io;
pub mod read_stylizing;
pub mod tui;
mod ui;

use crate::control::{handle_input, SearchPatternEdit, Update};
use anyhow::Result;
use app::App;
use event::{Event, EventHandler};
use ratatui::prelude::{CrosstermBackend, Terminal};
use std::env;
use tui::Tui;

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
    tui.draw(&mut app)?;

    // Start the main loop.
    while !app.quit {
        // Handle events.
        let updates: Update = handle_input(&app, &tui, tui.events.next()?);
        match updates {
            Update::None => continue, // no need to re-draw
            Update::ToggleUIMode => app.toggle_ui_mode(),
            Update::WindowResize(rect) => {
                todo!();
                //app.resized_update(rect);
            }
            Update::ScrollViewer(num) => {
                app.scroll(num, tui.size());
            }
            Update::Quit => {
                app.quit = true;
                break;
            }
            Update::SearchPanelFocus(focus) => {
                app.focus_search_panel(focus);
            }
            Update::SearchPanelInput(focus, input) => {
                match focus {
                    app::SearchPanelFocus::InputPattern => {
                        app.search_panel.input_pattern.input(input);
                    }
                    app::SearchPanelFocus::InputColor => {
                        _ = app.search_panel.input_color.input(input)
                    }
                    app::SearchPanelFocus::InputDistance => {
                        _ = app.search_panel.input_distance.input(input)
                    }
                    _ => {}
                };
            }
            Update::EditSearchPattern(edit) => match edit {
                SearchPatternEdit::Append(x) => app.append_search_pattern(x),
                SearchPatternEdit::Delete(index, pop) => {
                    if pop {
                        app.edit_search_pattern(index);
                    } else {
                        app.delete_search_pattern(index);
                    }
                }
            },
            Update::Msg(msg) => app.message(msg),
            Update::CycleSearchPattern(reverse) => app.cycle_patterns_list(reverse),
        };

        // Render the user interface.
        tui.draw(&mut app)?;
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
