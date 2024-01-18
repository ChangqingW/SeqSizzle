pub mod app;
pub mod control;
pub mod event;
pub mod io;
pub mod read_stylizing;
pub mod tui;
mod ui;

use crate::control::{handle_input, SearchPatternEdit, Update};
use anyhow::Result;
use app::{App, SearchPattern};
use event::{Event, EventHandler};
use ratatui::prelude::{CrosstermBackend, Terminal, Color};
use std::path::PathBuf;
use tui::Tui;
use clap::Parser;

/// A pager for viewing FASTQ files with fuzzy matching, allowing different adaptors to be colored differently.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The FASTQ file to view
    file: PathBuf,
    
    /// Start with 10x 3' kit adaptors:
    ///  - Patrial Read1: CTACACGACGCTCTTCCGATCT (and reverse complement)
    ///  - Partial TSO: AGATCGGAAGAGCGTCGTGTAG (and reverse complement)
    ///  - Poly(>10)A/T
    #[clap(long, verbatim_doc_comment)] // https://github.com/clap-rs/clap/issues/2389
    adapter_3p: bool,

    /// Start with 10x 5' kit adaptors
    ///  - Patrial Read1: CTACACGACGCTCTTCCGATCT (and reverse complement)
    ///  - TSO: TTTCTTATATGGG (and reverse complement)
    ///  - Patrial Read2: AGATCGGAAGAGCACACGTCTGAA (and reverse complement)
    ///  - Poly(>10)A/T
    #[clap(long, verbatim_doc_comment)]
    adapter_5p: bool,

    /// Start with patterns from a CSV file 
    /// Not yet implemented
    #[arg(short, long)]
    patterns: Option<PathBuf>,
}

fn main() -> Result<()> {

    let args = Args::parse();

    // add patterns based on command line arguments
    let mut patterns: Vec<SearchPattern> = Vec::new();
    if args.adapter_3p {
        patterns.extend_from_slice(&[
            SearchPattern::new("CTACACGACGCTCTTCCGATCT".to_string(), Color::Blue, 3, Some("R1")),
            SearchPattern::new("AGATCGGAAGAGCGTCGTGTAG".to_string(), Color::Green, 3, Some("TSO")),
            SearchPattern::new("TGGTATCAACGCAGAGTACATGGG".to_string(), Color::Red, 3, Some("R1 rev")),
            SearchPattern::new("CCCATGTACTCTGCGTTGATACCA".to_string(), Color::Yellow, 3, Some("TSO rev")),
            SearchPattern::new("TTTTTTTTTTTT".to_string(), Color::Gray, 0, None),
            SearchPattern::new("AAAAAAAAAAAA".to_string(), Color::Gray, 0, None),
        ]);
    }
    if args.adapter_5p {
        patterns.extend_from_slice(&[
            SearchPattern::new("CTACACGACGCTCTTCCGATCT".to_string(), Color::Blue, 3, Some("R1")),
            SearchPattern::new("TTTCTTATATGGG".to_string(), Color::Green, 2, Some("TSO")),
            SearchPattern::new("TGGTATCAACGCAGAGTACATGGG".to_string(), Color::Red, 3, Some("R1 rev")),
            SearchPattern::new("CCCATATAAGAAA".to_string(), Color::Yellow, 2, Some("TSO rev")),
            SearchPattern::new("AGATCGGAAGAGCACACGTCTGAA".to_string(), Color::Cyan, 3, Some("R2")),
            SearchPattern::new("TTCAGACGTGTGCTCTTCCGATCT".to_string(), Color::Magenta, 3, Some("R2 rev")),
            SearchPattern::new("TTTTTTTTTTTT".to_string(), Color::Gray, 0, None),
            SearchPattern::new("AAAAAAAAAAAA".to_string(), Color::Gray, 0, None),
        ]);
    }
    // TODO
    //if let Some(path) = args.patterns {
    //    // patterns.extend_from_slice();
    //}
    let mut app = App::new(&args.file, patterns);


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
                app.resized_update(rect);
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
            Update::Msg(msg) => app.set_message(msg),
            Update::CycleSearchPattern(reverse) => app.cycle_patterns_list(reverse),
        };

        // Render the user interface.
        tui.draw(&mut app)?;
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
