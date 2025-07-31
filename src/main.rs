pub mod app;
pub mod control;
pub mod event;
pub mod io;
pub mod read_stylizing;
pub mod search_panel;
pub mod tui;
pub mod match_summarizing;
mod ui;

use crate::control::{handle_input, SearchPatternEdit, Update};
use anyhow::Result;
use app::{App, SearchPattern, StylingConfig, QualityStyleMode};
use bio::io::fastq;
use clap::{Parser, Subcommand};
use event::{Event, EventHandler};
use ratatui::prelude::{Color, CrosstermBackend, Terminal};
use shadow_rs::shadow;
use std::path::PathBuf;
use tui::Tui;

shadow!(build);

/// A pager for viewing FASTQ and FASTA files with fuzzy matching, allowing different adaptors to be colored differently.
#[derive(Parser, Debug)]
#[command(author, about, long_about = None)]
#[command(version = build::CLAP_LONG_VERSION)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    /// The FASTQ or FASTA file to view (supports .fastq, .fasta, .fa, .fq and their .gz variants)
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

    /// Save the search panel to a CSV file before quitting.
    /// To be removed in the future since you can now hit
    /// Ctrl-S in the search panel to save the patterns.
    #[clap(short = 's', long = "save-patterns")]
    save_patterns_path: Option<PathBuf>,

    /// Enable italic styling for low quality bases (enabled by default)
    #[clap(long = "quality-italic", default_value = "true")]
    quality_italic: bool,

    /// Disable italic styling for low quality bases
    #[clap(long = "no-quality-italic", conflicts_with = "quality_italic")]
    no_quality_italic: bool,

    /// Quality threshold for styling (default: 10)
    #[clap(long = "quality-threshold", default_value = "10")]
    quality_threshold: u8,

    /// Enable background color styling based on quality scores.
    /// You will probably have a hard time distinguishing forground colors
    /// from background colors, so this is disabled by default.
    #[clap(long = "quality-colors")]
    quality_colors: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Summarize the reads with patterns specified by the --patterns argument or the adapter
    /// flags. Make sure you supply the flags BEFORE the subcommand, e.g. `./SeqSizzle my.fastq -p
    /// my_patterns.csv --adapter-3p summarize`.
    /// '..' indicats unmatched regions of positive length, 
    /// '-' indicates the patterns are overlapped, 
    /// print the number of reads that match each pattern combination in TSV format. 
    /// To be moved to the UI in the future.
    Summarize {
        /// Print the counts of each summarized catagory instead of the percentage
        #[clap(long)]
        counts: bool,
    }
}

fn create_styling_config(args: &Args) -> StylingConfig {
    // Determine if italic styling should be enabled
    let enable_italic = if args.no_quality_italic {
        false
    } else {
        args.quality_italic
    };
    
    // Determine quality style mode based on arguments
    let quality_style_mode = if args.quality_colors {
        if enable_italic {
            QualityStyleMode::Both
        } else {
            QualityStyleMode::Background
        }
    } else if enable_italic {
        QualityStyleMode::Italic
    } else {
        QualityStyleMode::None
    };
    
    // Only set threshold if we have some quality styling enabled
    let quality_threshold = if quality_style_mode != QualityStyleMode::None {
        Some(args.quality_threshold)
    } else {
        None
    };
    
    StylingConfig {
        quality_threshold,
        quality_style_mode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_args() -> Args {
        Args {
            file: PathBuf::new(),
            patterns_path: None,
            save_patterns_path: None,
            adapter_3p: false,
            adapter_5p: false,
            quality_italic: true,
            no_quality_italic: false,
            quality_threshold: 10,
            quality_colors: false,
            command: None,
        }
    }

    #[test]
    fn test_styling_config_default_italic() {
        let args = create_test_args();
        let config = create_styling_config(&args);
        
        assert_eq!(config.quality_threshold, Some(10));
        assert_eq!(config.quality_style_mode, QualityStyleMode::Italic);
    }

    #[test]
    fn test_styling_config_no_italic() {
        let mut args = create_test_args();
        args.no_quality_italic = true;
        args.quality_italic = false;
        
        let config = create_styling_config(&args);
        
        assert_eq!(config.quality_threshold, None);
        assert_eq!(config.quality_style_mode, QualityStyleMode::None);
    }

    #[test]
    fn test_styling_config_quality_colors() {
        let mut args = create_test_args();
        args.quality_colors = true;
        
        let config = create_styling_config(&args);
        
        assert_eq!(config.quality_threshold, Some(10));
        assert_eq!(config.quality_style_mode, QualityStyleMode::Both);
    }

    #[test]
    fn test_styling_config_quality_colors_no_italic() {
        let mut args = create_test_args();
        args.quality_colors = true;
        args.no_quality_italic = true;
        args.quality_italic = false;
        
        let config = create_styling_config(&args);
        
        assert_eq!(config.quality_threshold, Some(10));
        assert_eq!(config.quality_style_mode, QualityStyleMode::Background);
    }

    #[test]
    fn test_styling_config_custom_threshold() {
        let mut args = create_test_args();
        args.quality_threshold = 30;
        
        let config = create_styling_config(&args);
        
        assert_eq!(config.quality_threshold, Some(30));
        assert_eq!(config.quality_style_mode, QualityStyleMode::Italic);
    }
}

fn main() -> Result<()> {
    if !shadow_rs::git_clean() {
        print!(
            "Warning: built with dirty repo:\n{}",
            shadow_rs::git_status_file()
        );
    }

    let args = Args::parse();

    // add patterns based on command line arguments
    let mut patterns: Vec<SearchPattern> = Vec::new();
    if args.adapter_3p {
        patterns.extend_from_slice(&[
            SearchPattern::new("CTACACGACGCTCTTCCGATCT".to_string(), Color::Blue, 3, "R1"),
            SearchPattern::new("AGATCGGAAGAGCGTCGTGTAG".to_string(), Color::Green, 3, "TSO"),
            SearchPattern::new("TGGTATCAACGCAGAGTACATGGG".to_string(), Color::Red, 3, "R1 rev"),
            SearchPattern::new("CCCATGTACTCTGCGTTGATACCA".to_string(), Color::Yellow, 3, "TSO rev"),
            SearchPattern::new("TTTTTTTTTTTT".to_string(), Color::Gray, 1, ""),
            SearchPattern::new("AAAAAAAAAAAA".to_string(), Color::Gray, 1, ""),
        ]);
    }
    if args.adapter_5p {
        patterns.extend_from_slice(&[
            SearchPattern::new("CTACACGACGCTCTTCCGATCT".to_string(), Color::Blue, 3, "R1"),
            SearchPattern::new("TTTCTTATATGGG".to_string(), Color::Green, 2, "TSO"),
            SearchPattern::new("TGGTATCAACGCAGAGTACATGGG".to_string(), Color::Red, 3, "R1 rev"),
            SearchPattern::new("CCCATATAAGAAA".to_string(), Color::Yellow, 2, "TSO rev"),
            SearchPattern::new("AGATCGGAAGAGCACACGTCTGAA".to_string(), Color::Cyan, 3, "R2"),
            SearchPattern::new("TTCAGACGTGTGCTCTTCCGATCT".to_string(), Color::Magenta, 3, "R2 rev"),
            SearchPattern::new("TTTTTTTTTTTT".to_string(), Color::Gray, 1, ""),
            SearchPattern::new("AAAAAAAAAAAA".to_string(), Color::Gray, 1, ""),
        ]);
    }

    // add patterns from CSV file
    if let Some(ref path) = args.patterns_path {
        let err_str = "Error opening provided pattern CSV file";
        let mut reader = csv::Reader::from_path(path).expect(err_str);
        assert_eq!(
            reader
                .headers()
                .expect("Error reading pattern CSV file headers")
                .clone(),
            csv::StringRecord::from(vec!["pattern", "color", "editdistance", "comment"]),
            "Pattern CSV file headers must be: pattern,color,editdistance,comment"
        );
        reader.records().for_each(|record| {
            let record = record.expect(err_str);
            let color = record.get(1).expect(err_str);
            let editdistance = record.get(2).expect(err_str);
            let comment = record.get(3).expect(err_str).parse::<String>().unwrap();
            let pattern = SearchPattern::new(
                record.get(0).expect(err_str).to_string(),
                color.parse::<Color>().unwrap_or_else(|_| panic!("Error parsing pattern CSV file record color: {}", color)),
                editdistance.parse::<u8>().unwrap_or_else(|_| panic!("Error parsing pattern CSV file record editdistance: {}",
                        editdistance)),
                comment.as_str(),
            );
            patterns.push(pattern);
        });
    }

    if let Some(command) = args.command {
        match command {
            Commands::Summarize { counts } => {
                if patterns.is_empty() {
                    println!("Must specify --patterns or --adapter-3p or --adapter-5p to use the summarize subcommand, e.g. ./SeqSizzle my.fastq -p my_patterns.csv --adapter-3p summarize");
                    return Err(anyhow::anyhow!("No patterns to summarize with"));
                }
                // Read all records using SequenceReader
                use crate::io::SequenceReader;
                let mut reader = SequenceReader::from_path(&args.file)?;
                let mut records = Vec::new();
                let mut index = 0;
                while let Some(record) = reader.get_index(index)? {
                    records.push(record);
                    index += 1;
                }
                println!("number_of_read\tpattern_combination");
                print!(
                    "{}",
                    match_summarizing::fmt_summarised_reads(&match_summarizing::summarise_reads(
                        &records, &patterns, counts
                    ), counts)
                );
            }
        }
        return Ok(());
    }

    let mut app = App::new(&args.file, patterns)?;

    // Configure quality styling based on command line arguments
    app.styling_config = create_styling_config(&args);
    
    // Refresh the display to apply the new styling configuration
    app.update();

    // Initialize the terminal user interface.
    let backend = CrosstermBackend::new(std::io::stderr());
    let terminal = Terminal::new(backend)?;
    if crossterm::style::available_color_count() < 256 {
        app.set_message(String::from("Warning: your terminal does not support 256 colors"));
    }
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
            Update::SearchPanelFocusNext(reverse) => {
                app.search_panel.focus_next(reverse);
            }
            Update::SearchPanelInput(input) => {
                app.search_panel.handle_input(input);
            }
            Update::EditSearchPattern(edit) => match edit {
                SearchPatternEdit::Append(x) => app.append_search_pattern(x),
                SearchPatternEdit::Delete(index, pop) => {
                    if pop {
                        // pop current pattern into edit boxs
                        app.edit_search_pattern(index);
                        // focus on pattern edit box
                        app.search_panel.focus_next(false);
                    } else {
                        app.delete_search_pattern(index);
                    }
                }
            },
            Update::Msg(msg) => app.set_message(msg),
            Update::CycleSearchPattern(reverse) => app.cycle_patterns_list(reverse),
            Update::SaveFilePopupInput(input) => {
                app.search_panel.file_popup_input(input);
            }
            Update::ToggleFilePopup => {
                match app.mode {
                    app::UIMode::SearchPanel(false) => {
                        app.mode = app::UIMode::SearchPanel(true);
                    }
                    app::UIMode::SearchPanel(true) => {
                        app.search_panel.clear_file_save_popup();
                        app.mode = app::UIMode::SearchPanel(false);
                    }
                    _ => panic!("ToggleFilePopup called in non-search panel mode"),
                }
            }
            Update::ToggleQualityItalic => {
                app.toggle_quality_italic();
                app.update(); // Refresh display
            }
            Update::ToggleQualityBackground => {
                app.toggle_quality_background();
                app.update(); // Refresh display
            }
        };

        // Render the user interface.
        tui.draw(&mut app)?;
    }

    // Save the search panel to a CSV file
    if args.save_patterns_path.is_some() {
        let mut writer = csv::Writer::from_path(args.save_patterns_path.unwrap())?;
        writer
            .write_record(["pattern", "color", "editdistance", "comment"])
            .expect("Error writing pattern CSV file headers");
        app.search_patterns.iter().for_each(|pattern| {
            writer
                .write_record(&[
                    pattern.search_string.clone(),
                    pattern.color.to_string(),
                    pattern.edit_distance.to_string(),
                    pattern.comment.to_string(),
                ])
                .expect("Error writing pattern CSV file record");
        });
        writer.flush()?;
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
