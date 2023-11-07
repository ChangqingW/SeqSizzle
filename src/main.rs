pub mod fileInputs;
pub mod readStylizing;
pub mod app;

// Standard library imports
use std::env;
use std::io::{stdout, Result};

// Bioinformatics related imports
use bio::alignment::Alignment;
use bio::io::fastq;
use bio::io::fastq::FastqRead;
use bio::pattern_matching::myers::Myers;

// Crossterm related imports
use crossterm::{
    event::{self, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};

// Ratatui related imports
use ratatui::prelude::{CrosstermBackend, Line, Stylize, Terminal, Text, Span, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

fn highligh_matches(intervals: Vec<(usize, usize)>, input_string: String) -> Line<'static> {
    let mut result: Vec<Span> = Vec::new();
    let mut current_index = 0;

    for (start, end) in intervals {
        if current_index < start {
            result.push(input_string[current_index..start].to_string().into());
        }
        if end <= input_string.len() {
            result.push(Span::styled(input_string[start..end].to_string(), Style::new().green()));
        }
        current_index = end;
    }

    if current_index < input_string.len() {
        result.push(input_string[current_index..].to_string().into());
    }

    Line::from(result)
}

fn merge_intervals(mut intervals: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    // Sort intervals based on the start value
    intervals.sort_by(|a, b| a.0.cmp(&b.0));
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for interval in intervals {
        if let Some(last_merged) = merged.last_mut() {
            // If the current interval overlaps with the last merged interval, merge them
            if interval.0 <= last_merged.1 {
                last_merged.1 = interval.1.max(last_merged.1);
            } else {
                // If no overlap, add the current interval to the merged list
                merged.push(interval);
            }
        } else {
            // If merged list is empty, add the first interval
            merged.push(interval);
        }
    }
    merged
}

fn read_fastq_file(file_path: &str, x: usize) -> Vec<fastq::Record> {
    let mut reader = fastq::Reader::from_file(file_path).expect("Failed to open fastq file");
    let mut record = fastq::Record::new();
    let mut nb_reads = 0;
    let mut records = Vec::new();
    reader.read(&mut record).expect("Failed to parse record");
    while !record.is_empty() {
        nb_reads += 1;
        if nb_reads > x {
            break;
        }
        records.push(record.to_owned());
        reader.read(&mut record).expect("Failed to parse record");
    }
    records
}

fn record_to_lines(record: &fastq::Record) -> Vec<Line> {
    let mut result: Vec<Line> = Vec::new();
    let seq = String::from_utf8_lossy(record.seq()).to_string();

    let mut myers = Myers::<u64>::new(b"AGATCGGAAGAGCGTCGTGTAGAA");
    let mut aln = Alignment::default();
    let matches: Vec<(usize, usize)> = merge_intervals(
        myers
            .find_all(record.seq(), 3)
            .map(|(a, b, _)| (a, b))
            .collect(),
    );

    result.push(record.id().to_string().into());
    result.push(highligh_matches(matches, seq));
    result
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let filename = &args[1];
    let records: Vec<fastq::Record> = read_fastq_file(filename, 5);

    stdout().execute(EnterAlternateScreen)?;
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;

    loop {
        terminal.draw(|frame| {
            let text: Vec<Line> = record_to_lines(&records[0]);
            frame.render_widget(
                Paragraph::new(text)
                    .block(Block::default().borders(Borders::ALL))
                    .wrap(Wrap { trim: false }),
                frame.size(),
            );
        })?;

        if event::poll(std::time::Duration::from_millis(16))? {
            if let event::Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}
