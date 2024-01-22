pub mod app;
pub mod control;
pub mod event;
pub mod io;
pub mod read_stylizing;
pub mod tui;
pub mod search_panel;
mod ui;

use crate::control::{handle_input, SearchPatternEdit, Update};
use anyhow::Result;
use app::{App, SearchPattern};
use event::{Event, EventHandler};
use ratatui::prelude::{CrosstermBackend, Terminal, Color};
use std::path::PathBuf;
use tui::Tui;
use clap::Parser;
use shadow_rs::shadow;

shadow!(build);

/// A pager for viewing FASTQ files with fuzzy matching, allowing different adaptors to be colored differently.
#[derive(Parser, Debug)]
#[command(author, about, long_about = None)]
#[command(version = build::CLAP_LONG_VERSION)]
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
    /// Must have the following header: 
    /// pattern,color,editdistance,comment
    #[clap(short = 'p', long = "patterns", verbatim_doc_comment)]
    patterns_path: Option<PathBuf>,

    // TODO: move to SearchPanel
    /// Save the search panel to a CSV file before quitting. 
    /// To be moved to the search panel GUI in the future.
    #[clap(short = 's', long = "save-patterns")]
    save_patterns_path: Option<PathBuf>,
}

fn main() -> Result<()> {

    if !shadow_rs::git_clean() {
        print!("Warning: built with dirty repo:\n{}", shadow_rs::git_status_file());
    }

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

    // add patterns from CSV file
    if let Some(path) = args.patterns_path {
        let err_str = "Error opening provided pattern CSV file";
        let mut reader = csv::Reader::from_path(path).expect(err_str);
        assert_eq!(reader.headers().expect("Error reading pattern CSV file headers").clone(),
                   csv::StringRecord::from(vec!["pattern", "color", "editdistance", "comment"]),
                   "Pattern CSV file headers must be: pattern,color,editdistance,comment");
        reader.records().for_each(|record| {
            
            let record = record.expect(err_str);
            let color = record.get(1).expect(err_str);
            let editdistance = record.get(2).expect(err_str);
            let comment = record.get(3).expect(err_str).parse::<String>().unwrap();
            let pattern = SearchPattern::new(
                record.get(0).expect(err_str).to_string(),
                color.parse::<Color>().expect(format!("Error parsing pattern CSV file record color: {}", color).as_str()),
                editdistance.parse::<u8>().expect(format!("Error parsing pattern CSV file record editdistance: {}", editdistance).as_str()),
                if comment.is_empty() { None } else { Some(comment.as_str()) }
            );
            patterns.push(pattern);
        });
    }

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

    // Save the search panel to a CSV file
    if args.save_patterns_path.is_some() {
        let mut writer = csv::Writer::from_path(args.save_patterns_path.unwrap())?;
        writer.write_record(&["pattern", "color", "editdistance", "comment"]).expect("Error writing pattern CSV file headers");
        app.search_patterns.iter()
            .for_each(|pattern| {
                writer.write_record(&[
                    pattern.search_string.clone(),
                    pattern.color.to_string(),
                    pattern.edit_distance.to_string(),
                    pattern.comment.clone().unwrap_or("".to_string())
                ]).expect("Error writing pattern CSV file record");
            });
        writer.flush()?;
    }
    
    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
