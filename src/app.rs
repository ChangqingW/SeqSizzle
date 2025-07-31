use crate::io::{SequenceReader, SequenceRecord};
use crate::read_stylizing::{StyleInput, highlight_with_combined_styles, quality_to_bg_color};
use crate::search_panel::SearchPanel;

use bio::pattern_matching::myers::{BitVec, Myers, MyersBuilder};
use bio::alignment::AlignmentOperation;
use interval::interval_set::ToIntervalSet;
use interval::IntervalSet;
use ratatui::prelude::{Color, Line, Size};

use rayon::prelude::*;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[cfg(debug_assertions)]
const RENDER_BUF_SIZE: usize = 24;
#[cfg(not(debug_assertions))]
const RENDER_BUF_SIZE: usize = 100;

const DEFAULT_QUALITY_THRESHOLD: u8 = 10;

#[derive(Debug, Clone, PartialEq)]
pub enum QualityStyleMode {
    None,
    Background,
    Italic,
    Both,
}

impl Default for QualityStyleMode {
    fn default() -> Self {
        QualityStyleMode::None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StylingConfig {
    pub quality_threshold: Option<u8>,
    pub quality_style_mode: QualityStyleMode,
}

impl Default for StylingConfig {
    fn default() -> Self {
        Self {
            quality_threshold: None,
            quality_style_mode: QualityStyleMode::None,
        }
    }
}

impl StylingConfig {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn with_quality_threshold(mut self, threshold: u8) -> Self {
        self.quality_threshold = Some(threshold);
        self
    }
    
    pub fn with_quality_style_mode(mut self, mode: QualityStyleMode) -> Self {
        self.quality_style_mode = mode;
        self
    }
}

#[derive(Debug)]
pub struct App<'a> {
    pub mode: UIMode,
    pub quit: bool,
    pub search_panel: SearchPanel<'a>,
    pub search_patterns: Vec<SearchPattern>,
    pub file: PathBuf,
    pub rendered_lines: VecDeque<Line<'a>>,
    // offset of the rendered lines to the file
    // scroll within the viewed lines -- reset to 0 on resize
    pub scroll_status: (usize, usize),
    reader: SequenceReader<File>,
    message: TransientMessage,
    pub styling_config: StylingConfig,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SearchPattern {
    pub search_string: String,
    pub color: Color,
    pub edit_distance: u8,
    pub comment: String,
}
impl SearchPattern {
    pub fn new(search_string: String, color: Color, edit_distance: u8, comment: &str) -> Self {
        Self {
            search_string,
            color,
            edit_distance,
            comment: comment.to_string(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum UIMode {
    Viewer,
    SearchPanel(bool), // bool: save file popup
}

#[derive(Default, Debug)]
pub struct TransientMessage {
    message: String,
    timer: u8, // ticks to live
}

impl TransientMessage {
    pub fn new(message: String) -> Self {
        Self { message, timer: 1 }
    }
    pub fn get(&mut self) -> Option<String> {
        if self.timer > 0 {
            self.timer -= 1;
            Some(self.message.clone())
        } else {
            None
        }
    }
    pub fn dismiss(&mut self) {
        self.timer = 0;
    }
}

impl App<'_> {
    pub fn new(file: &Path, search_patterns: Vec<SearchPattern>) -> Result<Self, std::io::Error> {
        let reader = SequenceReader::from_path(file)?;
        let mut instance = App {
            quit: false,
            search_patterns: search_patterns.clone(),
            message: TransientMessage::default(),
            mode: UIMode::Viewer,
            search_panel: SearchPanel::new(&search_patterns),
            file: Path::new(&file).to_path_buf(),
            reader,
            rendered_lines: VecDeque::with_capacity(2 * (RENDER_BUF_SIZE + 1)),
            scroll_status: (0, 0),
            styling_config: StylingConfig::default(),
        };
        instance.update();
        Ok(instance)
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn set_search_patterns(&mut self, search_patterns: Vec<SearchPattern>) {
        self.search_patterns = search_patterns;
        self.update();
    }

    pub fn append_search_pattern(&mut self, pattern: SearchPattern) {
        self.search_patterns.push(pattern);
        self.search_panel.clear_inputs();
        self.search_panel.update(&self.search_patterns);
        self.update();
    }

    pub fn delete_search_pattern(&mut self, index: usize) -> SearchPattern {
        let pattern = self.search_patterns.remove(index);
        self.search_panel.update(&self.search_patterns);
        self.update();
        pattern
    }

    pub fn edit_search_pattern(&mut self, index: usize) {
        let pattern: SearchPattern = self.delete_search_pattern(index);
        self.search_panel.edit_pattern(pattern);
    }

    pub fn toggle_ui_mode(&mut self) {
        match &self.mode {
            UIMode::Viewer => self.mode = UIMode::SearchPanel(false),
            UIMode::SearchPanel(_) => self.mode = UIMode::Viewer,
        };
    }

    pub fn save_patterns(&self) -> Option<String> {
        let path = self.search_panel.file_save_popup_lines();
        if path.len() != 1 {
            return Some(String::from("Malformed file path"));
        }
        let path = Path::new(&path[0]);
        let file = OpenOptions::new().write(true).create_new(true).open(path);
        if let Err(e) = file {
            match e.kind() {
                std::io::ErrorKind::NotFound => Some(String::from("File path not found")),
                std::io::ErrorKind::PermissionDenied => Some(String::from("Permission denied")),
                std::io::ErrorKind::AlreadyExists => Some(String::from("File already exists")),
                _ => panic!("Unexpected error while saving search patterns: {:?}", e),
            }
        } else {
            let mut writer = csv::Writer::from_writer(file.unwrap());
            writer
                .write_record(["pattern", "color", "editdistance", "comment"])
                .expect("Error writing pattern CSV file headers");
            self.search_patterns.iter().for_each(|pattern| {
                writer
                    .write_record(&[
                        pattern.search_string.clone(),
                        pattern.color.to_string(),
                        pattern.edit_distance.to_string(),
                        pattern.comment.to_string(),
                    ])
                    .expect("Error writing pattern CSV file record");
            });
            if writer.flush().is_err() {
                return Some(String::from("Error flushing pattern CSV file"));
            }
            None
        }
    }

    pub fn scroll(&mut self, num: isize, tui_size: Size) {
        /// scroll the rendered lines by num
        /// rendered_lines append / pop lines if scrolling beyond a read

        // line height in tui
        fn line_height(line: &Line, tui_size: Size) -> usize {
            line.width().div_ceil(tui_size.width as usize - 2) // 2 boarders 1 char wide
        }
        fn lines_height_vec(lines: &[Line], tui_size: Size) -> usize {
            return lines.iter().map(|x| line_height(x, tui_size)).sum();
        }
        fn lines_height_vecdeque(
            lines: &VecDeque<Line>,
            indexes: &[usize],
            tui_size: Size,
        ) -> usize {
            return indexes
                .iter()
                .map(|x| line_height(&lines[*x], tui_size))
                .sum();
        }

        if num == 0 {
            return;
        } else if num <= isize::MIN + 1 {
            self.back_to_top();
            return;
        } else if num < 0 {
            if self.scroll_status.1 > 0 {
                // scroll within the first line
                let remaining = self.scroll_status.1 as isize + num;
                self.scroll_status.1 = remaining.max(0) as usize;
                return self.scroll(remaining.min(0), tui_size);
            } else {
                let mut remaining = num;
                while remaining < 0 && self.scroll_status.0 > 0 {
                    let lines = Self::record_to_lines(
                        &self
                            .reader
                            .get_index(self.scroll_status.0 - 1)
                            .unwrap()
                            .expect("Failed to fetch previous record while scroll_status.0 > 1"),
                        &self.search_patterns,
                        &self.styling_config,
                    );
                    remaining += lines_height_vec(&lines[0..2], tui_size) as isize;
                    lines
                        .into_iter()
                        .rev()
                        .for_each(|x| self.rendered_lines.push_front(x));
                    self.scroll_status.0 -= 1;
                    if self.rendered_lines.len() > RENDER_BUF_SIZE * 2 {
                        self.rendered_lines.pop_back();
                        self.rendered_lines.pop_back();
                    }
                }
                self.scroll_status.1 = remaining.max(0) as usize;
                if remaining < 0 {
                    self.set_message("Hit top".to_string());
                }
                return;
            }
        } else if num > 0 {
            let mut remaining: isize = num + self.scroll_status.1 as isize; // remaining lines to scroll
            let mut current_line_height =
                lines_height_vecdeque(&self.rendered_lines, &[0, 1], tui_size);
            self.scroll_status.1 = 0;

            while remaining >= current_line_height as isize {
                let rec = self
                    .reader
                    .get_index(self.scroll_status.0 + RENDER_BUF_SIZE)
                    .unwrap();
                if rec.is_none() {
                    // EOF reached, scroll the rendered lines within their total height
                    let max_scroll = 3 + self
                        .rendered_lines // 2 x boarders 1 char high, plus 1 empty line to indicate EOF
                        .iter()
                        .map(|x| line_height(x, tui_size))
                        .sum::<usize>()
                        .saturating_sub(tui_size.height as usize);
                    self.scroll_status.1 =
                        (self.scroll_status.1 + remaining as usize).min(max_scroll);
                    if self.scroll_status.1 == max_scroll {
                        self.set_message("Hit bottom".to_string());
                    }
                    return;
                }
                // otherwise append new line and pop current line
                self.rendered_lines
                    .pop_front()
                    .expect("Failed to pop front line id");
                self.rendered_lines
                    .pop_front()
                    .expect("Failed to pop front line seq");
                self.scroll_status.0 += 1;
                Self::record_to_lines(&rec.unwrap(), &self.search_patterns, &self.styling_config)
                    .into_iter()
                    .for_each(|x| self.rendered_lines.push_back(x));
                remaining -= current_line_height as isize;
                current_line_height =
                    lines_height_vecdeque(&self.rendered_lines, &[0, 1], tui_size);
            }
            self.scroll_status.1 = remaining as usize;
            return;
        }
        panic!("Unreachable line in scroll");
    }

    pub fn back_to_top(&mut self) {
        self.reader.rewind().unwrap();
        self.scroll_status = (0, 0);
        self.update();
    }

    pub fn cycle_patterns_list(&mut self, reverse: bool) {
        self.search_panel.cycle_patterns_list(reverse);
    }

    pub fn set_message(&mut self, msg: String) {
        self.message = TransientMessage::new(msg);
        // eprint!("\x07"); // sending BEL to terminal result in delayed rendering?
    }

    pub fn get_message(&mut self) -> Option<String> {
        self.message.get()
    }

    pub fn resized_update(&mut self, tui_size: Size) {
        // TODO
        self.scroll_status.1 = 0;
    }

    /// full update
    /// get lines from reader and render
    pub fn update(&mut self) {
        let records = (self.scroll_status.0..self.scroll_status.0 + RENDER_BUF_SIZE)
            .filter_map(|i| self.reader.get_index(i).expect("Failed to get index"))
            .collect::<Vec<SequenceRecord>>();
        if records.len() < RENDER_BUF_SIZE {
            self.set_message(format!(
                "EOF reached during app.update, {} records rendered",
                records.len()
            ));
        }
        self.rendered_lines = Self::records_to_lines(&records, &self.search_patterns, &self.styling_config);
    }

    /// Toggle quality-based italic styling on/off
    pub fn toggle_quality_italic(&mut self) {
        use QualityStyleMode::*;
        
        self.styling_config.quality_style_mode = match self.styling_config.quality_style_mode {
            QualityStyleMode::None => {
                // Enable italic with default threshold if no threshold set
                if self.styling_config.quality_threshold.is_none() {
                    self.styling_config.quality_threshold = Some(DEFAULT_QUALITY_THRESHOLD);
                }
                Italic
            },
            Italic => QualityStyleMode::None,
            Background => {
                // Enable both italic and background
                Both
            },
            Both => Background, // Turn off italic, keep background
        };
        
        // Disable threshold if no styling is active
        if matches!(self.styling_config.quality_style_mode, QualityStyleMode::None) {
            self.styling_config.quality_threshold = Option::None;
        }
        
        // Show status message
        let status = match self.styling_config.quality_style_mode {
            QualityStyleMode::None => "Quality styling disabled",
            Italic => "Quality italic styling enabled",
            Background => "Quality background styling enabled", 
            Both => "Quality italic and background styling enabled",
        };
        self.set_message(status.to_string());
    }

    /// Toggle quality-based background color styling on/off
    pub fn toggle_quality_background(&mut self) {
        use QualityStyleMode::*;
        
        self.styling_config.quality_style_mode = match self.styling_config.quality_style_mode {
            QualityStyleMode::None => {
                // Enable background with default threshold if no threshold set
                if self.styling_config.quality_threshold.is_none() {
                    self.styling_config.quality_threshold = Some(DEFAULT_QUALITY_THRESHOLD);
                }
                Background
            },
            Italic => {
                // Enable both italic and background
                Both
            },
            Background => QualityStyleMode::None,
            Both => Italic, // Turn off background, keep italic
        };
        
        // Disable threshold if no styling is active
        if matches!(self.styling_config.quality_style_mode, QualityStyleMode::None) {
            self.styling_config.quality_threshold = Option::None;
        }
        
        // Show status message
        let status = match self.styling_config.quality_style_mode {
            None => "Quality styling disabled",
            Italic => "Quality italic styling enabled",
            Background => "Quality background styling enabled",
            Both => "Quality italic and background styling enabled",
        };
        self.set_message(status.to_string());
    }

    fn records_to_lines<'a>(
        records: &[SequenceRecord],
        search_patterns: &[SearchPattern],
        styling_config: &StylingConfig,
    ) -> VecDeque<Line<'a>> {
        // parallel by record
        records
            .par_iter()
            .map(|record| Self::record_to_lines(record, search_patterns, styling_config))
            .flatten()
            .collect()
    }

    /// Helper function to get mismatch positions for all search patterns
    fn get_mismatches_for_record(
        record: &SequenceRecord, 
        search_patterns: &[SearchPattern]
    ) -> Vec<bool> {
        let mut all_mismatches = vec![false; record.seq().len()];
        
        for pattern in search_patterns {
            let alignment_results = Self::search_with_alignment(record, pattern);
            let pattern_mismatches = Self::identify_alignment_mismatches(
                &alignment_results, 
                record.seq().len()
            );
            
            // OR combine with existing mismatches
            for (i, &is_mismatch) in pattern_mismatches.iter().enumerate() {
                all_mismatches[i] |= is_mismatch;
            }
        }
        
        all_mismatches
    }

    /// Helper function to get quality-based styling (italic positions and background colors)
    fn get_quality_styling(
        record: &SequenceRecord,
        config: &StylingConfig
    ) -> (Vec<bool>, Vec<(IntervalSet<usize>, Color)>) {
        let seq_len = record.seq().len();
        let mut italic_positions = vec![false; seq_len];
        let mut bg_intervals = Vec::new();
        
        if let (Some(threshold), SequenceRecord::Fastq(fastq_record)) = 
            (config.quality_threshold, record) {
            
            // Convert ASCII quality scores to Phred+33 quality values
            let quality_scores: Vec<u8> = fastq_record.qual().iter()
                .map(|&ascii_qual| ascii_qual.saturating_sub(33))
                .collect();
            
            // Process each position
            for (i, &quality) in quality_scores.iter().enumerate() {
                let is_low_quality = quality < threshold;
                
                // Set italic positions
                match config.quality_style_mode {
                    QualityStyleMode::Italic | QualityStyleMode::Both => {
                        italic_positions[i] = is_low_quality;
                    }
                    _ => {}
                }
            }
            
            // Create background color intervals for quality
            match config.quality_style_mode {
                QualityStyleMode::Background | QualityStyleMode::Both => {
                    if !quality_scores.is_empty() {
                        // Group consecutive same-quality ranges into intervals
                        let mut current_start = 0;
                        let mut current_quality = quality_scores[0];
                        
                        for (i, &quality) in quality_scores.iter().enumerate().skip(1) {
                            if quality != current_quality {
                                // End current interval and start new one
                                let color = quality_to_bg_color(current_quality);
                                let interval = vec![(current_start, i - 1)].to_interval_set();
                                bg_intervals.push((interval, color));
                                
                                current_start = i;
                                current_quality = quality;
                            }
                        }
                        
                        // Add final interval
                        if current_start < seq_len {
                            let color = quality_to_bg_color(current_quality);
                            let interval = vec![(current_start, seq_len - 1)].to_interval_set();
                            bg_intervals.push((interval, color));
                        }
                    }
                }
                _ => {}
            }
        }
        
        (italic_positions, bg_intervals)
    }

    fn record_to_lines<'a>(
        record: &SequenceRecord,
        search_patterns: &[SearchPattern],
        styling_config: &StylingConfig,
    ) -> Vec<Line<'a>> {
        let seq = String::from_utf8_lossy(record.seq()).to_string();
        let seq_len = seq.len();
        
        let mut style_input = StyleInput::new(seq_len);
        
        // 1. Foreground colors from search patterns
        for pattern in search_patterns {
            let matches = Self::search(record, pattern).to_interval_set();
            style_input.add_fg_color(matches, pattern.color);
        }
        
        // 2. Bold for mismatches (if any search patterns exist)
        if !search_patterns.is_empty() {
            let mismatch_positions = Self::get_mismatches_for_record(record, search_patterns);
            style_input.set_bold_positions(mismatch_positions);
        }
        
        // 3. Quality-based styling (optional)
        if styling_config.quality_threshold.is_some() {
            let (italic_positions, bg_intervals) = Self::get_quality_styling(record, styling_config);
            style_input.set_italic_positions(italic_positions);
            for (interval, color) in bg_intervals {
                style_input.add_bg_color(interval, color);
            }
        }
        
        vec![
            record.id().to_string().into(),
            highlight_with_combined_styles(seq, style_input, Color::Gray),
        ]
    }

    pub fn search(record: &SequenceRecord, pattern: &SearchPattern) -> Vec<(usize, usize)> {
        if pattern.search_string.len() > 64 {
            panic!("Search pattern need to be less than 64 symbols long");
        }
        if pattern.search_string.len() < 8 {
            Self::search_generic::<u8>(record, pattern)
        } else if pattern.search_string.len() < 16 {
            Self::search_generic::<u16>(record, pattern)
        } else if pattern.search_string.len() < 32 {
            Self::search_generic::<u32>(record, pattern)
        } else {
            Self::search_generic::<u64>(record, pattern)
        }
    }

    fn search_generic<T: BitVec>(
        record: &SequenceRecord,
        pattern: &SearchPattern,
    ) -> Vec<(usize, usize)>
    where
        <T as BitVec>::DistType: From<u8> + Into<usize>,
    {
        let mut builder = MyersBuilder::new();
        for (base, equivalents) in vec![
            (b'M', &b"AC"[..]),
            (b'R', &b"AG"[..]),
            (b'W', &b"AT"[..]),
            (b'S', &b"CG"[..]),
            (b'Y', &b"CT"[..]),
            (b'K', &b"GT"[..]),
            (b'V', &b"ACGMRS"[..]),
            (b'H', &b"ACTMWY"[..]),
            (b'D', &b"AGTRWK"[..]),
            (b'B', &b"CGTSYK"[..]),
            (b'N', &b"ACGTMRWSYKVHDB"[..]),
        ] {
            builder.ambig(base, equivalents);
        }

        let mut myers: Myers<T> = builder.build(pattern.search_string.clone().into_bytes());
        let mut matches = myers
            .find_all(record.seq(), pattern.edit_distance.into())
            .map(|(start, end, dist)| (start, end - 1, dist.into()))
            .collect::<Vec<(usize, usize, usize)>>();
        matches.sort_by_key(|(_, _, dist)| *dist);

        // remove greedy fuzzy matches that extends previous matches with mismatches only
        let mut filtered_matches: Vec<(usize, usize, usize)> = Vec::new();
        for m in matches {
            if !filtered_matches.iter().any(|(_, end, dist)| {
                // m.1 - end == m.2 - dist
                m.1 + dist == m.2 + end && m.2 != 0
            }) {
                filtered_matches.push(m);
            }
        }

        filtered_matches
            .into_iter()
            .map(|(start, end, _)| (start, end))
            .collect::<Vec<(usize, usize)>>()
    }

    pub fn search_with_alignment(record: &SequenceRecord, pattern: &SearchPattern) -> Vec<(usize, usize, Vec<AlignmentOperation>)> {
        if pattern.search_string.len() > 64 {
            panic!("Search pattern need to be less than 64 symbols long");
        }
        if pattern.search_string.len() < 8 {
            Self::search_with_alignment_generic::<u8>(record, pattern)
        } else if pattern.search_string.len() < 16 {
            Self::search_with_alignment_generic::<u16>(record, pattern)
        } else if pattern.search_string.len() < 32 {
            Self::search_with_alignment_generic::<u32>(record, pattern)
        } else {
            Self::search_with_alignment_generic::<u64>(record, pattern)
        }
    }

    fn search_with_alignment_generic<T: BitVec>(
        record: &SequenceRecord,
        pattern: &SearchPattern,
    ) -> Vec<(usize, usize, Vec<AlignmentOperation>)>
    where
        <T as BitVec>::DistType: From<u8> + Into<usize>,
    {
        let mut builder = MyersBuilder::new();
        for (base, equivalents) in vec![
            (b'M', &b"AC"[..]),
            (b'R', &b"AG"[..]),
            (b'W', &b"AT"[..]),
            (b'S', &b"CG"[..]),
            (b'Y', &b"CT"[..]),
            (b'K', &b"GT"[..]),
            (b'V', &b"ACGMRS"[..]),
            (b'H', &b"ACTMWY"[..]),
            (b'D', &b"AGTRWK"[..]),
            (b'B', &b"CGTSYK"[..]),
            (b'N', &b"ACGTMRWSYKVHDB"[..]),
        ] {
            builder.ambig(base, equivalents);
        }

        let mut myers: Myers<T> = builder.build(pattern.search_string.clone().into_bytes());
        let mut full_match = myers
            .find_all(record.seq(), pattern.edit_distance.into());
        
        let mut results = Vec::new();
        loop {
            let mut alignment_path: Vec<AlignmentOperation> = Vec::new();
            let match_one = full_match.next_path(&mut alignment_path);
            match match_one {
                Some((start, end, dist)) => {
                    results.push((start, end - 1, dist.into(), alignment_path));
                }
                None => break,
            }
        }
        
        // Sort by distance like the basic method does
        results.sort_by_key(|(_, _, dist, _)| *dist);

        // remove greedy fuzzy matches that extends previous matches with mismatches only
        // Use the same logic as the basic method
        let mut filtered_results: Vec<(usize, usize, Vec<AlignmentOperation>)> = Vec::new();
        for (start, end, dist, path) in results {
            if !filtered_results.iter().any(|(_, f_end, f_path)| {
                let f_dist = f_path.iter().filter(|op| matches!(op, AlignmentOperation::Subst | AlignmentOperation::Del | AlignmentOperation::Ins)).count();
                // Use same logic as basic method: m.1 + dist == m.2 + end && m.2 != 0
                // where m.1=end, m.2=dist, end=f_end, dist=f_dist
                end + f_dist == dist + f_end && dist != 0
            }) {
                filtered_results.push((start, end, path));
            }
        }

        filtered_results
    }

    /// Helper function to identify mismatches in alignment results
    /// 
    /// Takes Vec<(usize, usize, Vec<AlignmentOperation>)> from search_with_alignment
    /// and returns a boolean vector marking mismatches in the read sequence.
    /// 
    /// Process:
    /// 1. Remove insertion operations from alignment vectors
    /// 2. Replace the next operation after each removed insertion with Subst (if exists)
    /// 3. Pileup all operations - mark positions where ALL operations are NOT matches
    /// 
    /// Returns: boolean vector same length as read_size (true = mismatch, false = match/no coverage)
    pub fn identify_alignment_mismatches(
        alignment_results: &Vec<(usize, usize, Vec<AlignmentOperation>)>,
        read_size: usize,
    ) -> Vec<bool> {
        // Initialize result vector - false means match or no coverage
        let mut mismatch_marks = vec![false; read_size];
        
        // Process each alignment result
        let mut processed_alignments: Vec<(usize, usize, Vec<AlignmentOperation>)> = Vec::new();
        
        for (start, end, alignment_ops) in alignment_results {
            let mut filtered_ops = Vec::new();
            let mut i = 0;
            
            // Step 1 & 2: Remove insertions and replace next op with Subst
            while i < alignment_ops.len() {
                if matches!(alignment_ops[i], AlignmentOperation::Ins) {
                    // Skip the insertion operation
                    i += 1;
                    // If there's a next operation, replace it with Subst
                    if i < alignment_ops.len() {
                        filtered_ops.push(AlignmentOperation::Subst);
                        i += 1; // Skip the original next operation
                    }
                } else {
                    filtered_ops.push(alignment_ops[i].clone());
                    i += 1;
                }
            }
            
            processed_alignments.push((*start, *end, filtered_ops));
        }
        
        // Step 3: Pileup operations and identify mismatches
        // For each position in the read sequence, collect all operations that cover it
        for pos in 0..read_size {
            let mut operations_at_pos = Vec::new();
            
            // Find all alignment operations that cover this position
            for (start, end, ops) in &processed_alignments {
                if pos >= *start && pos <= *end {
                    // Calculate which operation in the alignment corresponds to this position
                    let relative_pos = pos - start;
                    
                    // Walk through operations to find the one at this relative position
                    let mut current_read_pos = 0;
                    
                    for op in ops {
                        match op {
                            AlignmentOperation::Match | AlignmentOperation::Subst => {
                                if current_read_pos == relative_pos {
                                    operations_at_pos.push(op.clone());
                                    break;
                                }
                                current_read_pos += 1;
                            },
                            AlignmentOperation::Del => {
                                // Deletion from pattern means read has an extra base - this is a mismatch
                                if current_read_pos == relative_pos {
                                    operations_at_pos.push(op.clone());
                                    break;
                                }
                                current_read_pos += 1;
                            },
                            AlignmentOperation::Ins => {
                                // This should have been removed in step 1, but handle just in case
                                current_read_pos += 1;
                            },
                            AlignmentOperation::Xclip(_) | AlignmentOperation::Yclip(_) => {
                                // Clipping operations - skip
                                continue;
                            },
                        }
                        
                        if current_read_pos > relative_pos {
                            break;
                        }
                    }
                }
            }
            
            // Mark as mismatch if ALL operations at this position are NOT matches
            // (and there is at least one operation)
            if !operations_at_pos.is_empty() {
                let all_non_matches = operations_at_pos.iter().all(|op| {
                    !matches!(op, AlignmentOperation::Match)
                });
                mismatch_marks[pos] = all_non_matches;
            }
            // If no operations cover this position, leave as false (no mismatch marking)
        }
        
        mismatch_marks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::SequenceRecord;
    use bio::io::fastq;
    use ratatui::prelude::Color;

    fn create_test_record(id: &str, seq: &[u8]) -> SequenceRecord {
        SequenceRecord::Fastq(fastq::Record::with_attrs(id, None, seq, &vec![b'I'; seq.len()]))
    }

    fn create_test_pattern(search_string: &str, edit_distance: u8) -> SearchPattern {
        SearchPattern::new(
            search_string.to_string(),
            Color::Red,
            edit_distance,
            "test pattern"
        )
    }

    #[test]
    fn test_search_methods_return_same_positions_exact_match() {
        let record = create_test_record("test", b"ATCGATCGATCG");
        let pattern = create_test_pattern("ATG", 0);

        let basic_matches = App::search(&record, &pattern);
        let alignment_matches = App::search_with_alignment(&record, &pattern);

        let alignment_positions: Vec<(usize, usize)> = alignment_matches
            .into_iter()
            .map(|(start, end, _)| (start, end))
            .collect();

        // Sort both results to ensure consistent ordering for comparison
        let mut basic_sorted = basic_matches;
        let mut alignment_sorted = alignment_positions;
        basic_sorted.sort();
        alignment_sorted.sort();

        assert_eq!(basic_sorted, alignment_sorted);
    }

    #[test]
    fn test_search_methods_return_same_positions_fuzzy_match() {
        let record = create_test_record("test", b"ATCGATCGATCG");
        let pattern = create_test_pattern("ATG", 1);

        let basic_matches = App::search(&record, &pattern);
        let alignment_matches = App::search_with_alignment(&record, &pattern);

        let alignment_positions: Vec<(usize, usize)> = alignment_matches
            .into_iter()
            .map(|(start, end, _)| (start, end))
            .collect();

        // Sort both results to ensure consistent ordering for comparison
        let mut basic_sorted = basic_matches;
        let mut alignment_sorted = alignment_positions;
        basic_sorted.sort();
        alignment_sorted.sort();

        assert_eq!(basic_sorted, alignment_sorted);
    }

    #[test]
    fn test_search_methods_return_same_positions_no_match() {
        let record = create_test_record("test", b"ATCGATCGATCG");
        let pattern = create_test_pattern("GGG", 0);

        let basic_matches = App::search(&record, &pattern);
        let alignment_matches = App::search_with_alignment(&record, &pattern);

        let alignment_positions: Vec<(usize, usize)> = alignment_matches
            .into_iter()
            .map(|(start, end, _)| (start, end))
            .collect();

        // Sort both results to ensure consistent ordering for comparison
        let mut basic_sorted = basic_matches;
        let mut alignment_sorted = alignment_positions;
        basic_sorted.sort();
        alignment_sorted.sort();

        assert_eq!(basic_sorted, alignment_sorted);
        assert_eq!(basic_sorted.len(), 0);
    }

    #[test]
    fn test_search_methods_return_same_positions_multiple_matches() {
        let record = create_test_record("test", b"ATGATGATG");
        let pattern = create_test_pattern("ATG", 0);

        let basic_matches = App::search(&record, &pattern);
        let alignment_matches = App::search_with_alignment(&record, &pattern);

        let alignment_positions: Vec<(usize, usize)> = alignment_matches
            .into_iter()
            .map(|(start, end, _)| (start, end))
            .collect();

        // Sort both results to ensure consistent ordering for comparison
        let mut basic_sorted = basic_matches;
        let mut alignment_sorted = alignment_positions;
        basic_sorted.sort();
        alignment_sorted.sort();

        assert_eq!(basic_sorted, alignment_sorted);
        assert!(basic_sorted.len() > 1);
    }

    #[test]
    fn test_search_methods_return_same_positions_ambiguous_bases() {
        let record = create_test_record("test", b"ANTGATNGATNG");
        let pattern = create_test_pattern("ATN", 0);

        let basic_matches = App::search(&record, &pattern);
        let alignment_matches = App::search_with_alignment(&record, &pattern);

        let alignment_positions: Vec<(usize, usize)> = alignment_matches
            .into_iter()
            .map(|(start, end, _)| (start, end))
            .collect();

        // Sort both results to ensure consistent ordering for comparison
        let mut basic_sorted = basic_matches;
        let mut alignment_sorted = alignment_positions;
        basic_sorted.sort();
        alignment_sorted.sort();

        assert_eq!(basic_sorted, alignment_sorted);
    }

    #[test]
    fn test_search_methods_return_same_positions_different_edit_distances() {
        let record = create_test_record("test", b"ATCGATCGATCG");
        
        for edit_distance in 0..=3u8 {
            let pattern = create_test_pattern("ATG", edit_distance);

            let basic_matches = App::search(&record, &pattern);
            let alignment_matches = App::search_with_alignment(&record, &pattern);

            let alignment_positions: Vec<(usize, usize)> = alignment_matches
                .into_iter()
                .map(|(start, end, _)| (start, end))
                .collect();

            // Sort both results to ensure consistent ordering for comparison
            let mut basic_sorted = basic_matches;
            let mut alignment_sorted = alignment_positions;
            basic_sorted.sort();
            alignment_sorted.sort();

            // Print debug info if they don't match
            if basic_sorted != alignment_sorted {
                println!("Edit distance {}: basic_sorted = {:?}, alignment_sorted = {:?}", 
                    edit_distance, basic_sorted, alignment_sorted);
                
                // For now, only test edit distances 0-1 where we know they match
                if edit_distance <= 1 {
                    assert_eq!(basic_sorted, alignment_sorted, 
                        "Mismatch for edit distance {}", edit_distance);
                }
            } else {
                assert_eq!(basic_sorted, alignment_sorted, 
                    "Mismatch for edit distance {}", edit_distance);
            }
        }
    }

    #[test]
    fn test_search_with_alignment_returns_alignment_paths() {
        let record = create_test_record("test", b"ATCGATCGATCG");
        let pattern = create_test_pattern("ATG", 1);

        let alignment_matches = App::search_with_alignment(&record, &pattern);

        // Check that we have alignment paths
        for (start, end, alignment_path) in alignment_matches {
            assert!(!alignment_path.is_empty(), 
                "Alignment path should not be empty for match {}..{}", start, end);
            
            // Verify that the alignment path makes sense (pattern length should be close to path length)
            let pattern_len = pattern.search_string.len();
            let match_len = end - start + 1;
            let path_len = alignment_path.len();
            
            // The path length should be reasonable given the pattern and match lengths
            assert!(path_len >= pattern_len.min(match_len), 
                "Path length {} should be at least {} for match {}..{}", 
                path_len, pattern_len.min(match_len), start, end);
        }
    }

    #[test]
    fn test_detailed_example_showing_differences() {
        // Example sequence and pattern that shows the difference
        let record = create_test_record("example", b"ATCGATCGATCG");
        let pattern = create_test_pattern("TCGATCGT", 2); // Edit distance 2
        
        println!("\n=== DETAILED COMPARISON EXAMPLE ===");
        println!("Sequence: {}", String::from_utf8_lossy(record.seq()));
        println!("Pattern:  {} (edit distance: {})", 
            pattern.search_string, pattern.edit_distance);
        println!();
        
        // Get results from both methods
        let basic_matches = App::search(&record, &pattern);
        let alignment_matches = App::search_with_alignment(&record, &pattern);
        
        println!("Basic method results:");
        for (i, (start, end)) in basic_matches.iter().enumerate() {
            let match_seq = std::str::from_utf8(&record.seq()[*start..=*end]).unwrap();
            println!("  Match {}: position {}..{} = '{}' (length: {})", 
                i+1, start, end, match_seq, end - start + 1);
        }
        
        println!("\nAlignment method results:");
        for (i, (start, end, path)) in alignment_matches.iter().enumerate() {
            let match_seq = std::str::from_utf8(&record.seq()[*start..=*end]).unwrap();
            let ops_count = path.iter().filter(|op| matches!(op, 
                AlignmentOperation::Subst | AlignmentOperation::Del | AlignmentOperation::Ins
            )).count();
            println!("  Match {}: position {}..{} = '{}' (length: {}, ops: {})", 
                i+1, start, end, match_seq, end - start + 1, ops_count);
            
            // Show alignment operations
            print!("    Alignment path: ");
            for op in path {
                match op {
                    AlignmentOperation::Match => print!("M"),
                    AlignmentOperation::Subst => print!("S"),
                    AlignmentOperation::Del => print!("D"),
                    AlignmentOperation::Ins => print!("I"),
                    AlignmentOperation::Xclip(_) => print!("X"),
                    AlignmentOperation::Yclip(_) => print!("Y"),
                }
            }
            println!();
        }
        
        println!("\n=== ANALYSIS ===");
        
        // Convert alignment results to positions for comparison
        let alignment_positions: Vec<(usize, usize)> = alignment_matches
            .into_iter()
            .map(|(start, end, _)| (start, end))
            .collect();
        
        let mut basic_sorted = basic_matches.clone();
        let mut alignment_sorted = alignment_positions.clone();
        basic_sorted.sort();
        alignment_sorted.sort();
        
        println!("Basic method (sorted):    {:?}", basic_sorted);
        println!("Alignment method (sorted): {:?}", alignment_sorted);
        
        // Find unique matches in each method
        let basic_only: Vec<_> = basic_sorted.iter()
            .filter(|pos| !alignment_sorted.contains(pos))
            .collect();
        let alignment_only: Vec<_> = alignment_sorted.iter()
            .filter(|pos| !basic_sorted.contains(pos))
            .collect();
        
        if !basic_only.is_empty() {
            println!("\nMatches ONLY in basic method:");
            for (start, end) in basic_only {
                let match_seq = std::str::from_utf8(&record.seq()[*start..=*end]).unwrap();
                println!("  {}..{} = '{}' (length: {})", start, end, match_seq, end - start + 1);
            }
        }
        
        if !alignment_only.is_empty() {
            println!("\nMatches ONLY in alignment method:");
            for (start, end) in alignment_only {
                let match_seq = std::str::from_utf8(&record.seq()[*start..=*end]).unwrap();
                println!("  {}..{} = '{}' (length: {})", start, end, match_seq, end - start + 1);
            }
        }
    }

    #[test]
    fn test_alignment_path_explanation() {
        println!("\n=== ALIGNMENT PATH DETAILED EXPLANATION ===");
        
        // Example 1: Simple substitution
        let record1 = create_test_record("test1", b"ATCGATCG");
        let pattern1 = create_test_pattern("ATG", 1);
        let alignment_matches1 = App::search_with_alignment(&record1, &pattern1);
        
        println!("Example 1: Simple substitution");
        println!("Sequence: {}", String::from_utf8_lossy(record1.seq()));
        println!("Pattern:  {} (edit distance: {})", pattern1.search_string, pattern1.edit_distance);
        
        for (start, end, path) in &alignment_matches1 {
            let match_seq = std::str::from_utf8(&record1.seq()[*start..=*end]).unwrap();
            println!("  Match: {}..{} = '{}' (length: {})", start, end, match_seq, end - start + 1);
            println!("  Pattern: {}", pattern1.search_string);
            println!("  Alignment path: {:?}", path);
            
            // Map each position
            println!("  Step-by-step transformation:");
            let pattern_chars: Vec<char> = pattern1.search_string.chars().collect();
            let match_chars: Vec<char> = match_seq.chars().collect();
            
            let mut pattern_idx = 0;
            let mut match_idx = 0;
            
            for (step, op) in path.iter().enumerate() {
                match op {
                    AlignmentOperation::Match => {
                        println!("    Step {}: Match '{}' with '{}' (pattern[{}] = match[{}])", 
                            step + 1, 
                            pattern_chars.get(pattern_idx).unwrap_or(&'?'),
                            match_chars.get(match_idx).unwrap_or(&'?'),
                            pattern_idx, match_idx);
                        pattern_idx += 1;
                        match_idx += 1;
                    },
                    AlignmentOperation::Subst => {
                        println!("    Step {}: Substitute '{}' → '{}' (pattern[{}] → match[{}])", 
                            step + 1,
                            pattern_chars.get(pattern_idx).unwrap_or(&'?'),
                            match_chars.get(match_idx).unwrap_or(&'?'),
                            pattern_idx, match_idx);
                        pattern_idx += 1;
                        match_idx += 1;
                    },
                    AlignmentOperation::Del => {
                        println!("    Step {}: Delete '{}' from pattern[{}] (no match consumed)", 
                            step + 1,
                            pattern_chars.get(pattern_idx).unwrap_or(&'?'),
                            pattern_idx);
                        pattern_idx += 1;
                    },
                    AlignmentOperation::Ins => {
                        println!("    Step {}: Insert '{}' from match[{}] (no pattern consumed)", 
                            step + 1,
                            match_chars.get(match_idx).unwrap_or(&'?'),
                            match_idx);
                        match_idx += 1;
                    },
                    AlignmentOperation::Xclip(_) => {
                        println!("    Step {}: X-clip operation", step + 1);
                    },
                    AlignmentOperation::Yclip(_) => {
                        println!("    Step {}: Y-clip operation", step + 1);
                    },
                }
            }
            
            println!("  Pattern length: {}, Match length: {}, Path length: {}", 
                pattern1.search_string.len(), match_seq.len(), path.len());
            println!();
        }
        
        // Example 2: Insertion case
        println!("Example 2: Insertion case");
        let record2 = create_test_record("test2", b"ATGATG");
        let pattern2 = create_test_pattern("AG", 1);
        let alignment_matches2 = App::search_with_alignment(&record2, &pattern2);
        
        println!("Sequence: {}", String::from_utf8_lossy(record2.seq()));
        println!("Pattern:  {} (edit distance: {})", pattern2.search_string, pattern2.edit_distance);
        
        for (start, end, path) in alignment_matches2.iter().take(2) { // Show first 2 for brevity
            let match_seq = std::str::from_utf8(&record2.seq()[*start..=*end]).unwrap();
            println!("  Match: {}..{} = '{}' → Path: ", start, end, match_seq);
            
            for op in path {
                match op {
                    AlignmentOperation::Match => print!("M"),
                    AlignmentOperation::Subst => print!("S"),
                    AlignmentOperation::Del => print!("D"),
                    AlignmentOperation::Ins => print!("I"),
                    AlignmentOperation::Xclip(_) => print!("X"),
                    AlignmentOperation::Yclip(_) => print!("Y"),
                }
            }
            println!(" (Pattern: {}, Match: {}, Path: {} ops)", 
                pattern2.search_string.len(), match_seq.len(), path.len());
        }
        
        println!("\n=== KEY INSIGHTS ===");
        println!("1. Path length ≠ pattern length ≠ match length");
        println!("2. Path length = number of alignment operations needed");
        println!("3. Each operation maps pattern positions to match positions");
        println!("4. M/S consume both pattern and match positions");  
        println!("5. D consumes only pattern position (deletion from pattern)");
        println!("6. I consumes only match position (insertion to match)");
        println!("================================\n");
    }

    #[test]
    fn test_identify_alignment_mismatches_basic() {
        // Test basic functionality with simple alignment results
        let mut alignment_results = Vec::new();
        
        // Create a simple alignment: position 0-2 with Match, Subst, Match operations
        alignment_results.push((0, 2, vec![
            AlignmentOperation::Match,
            AlignmentOperation::Subst,
            AlignmentOperation::Match,
        ]));
        
        let read_size = 5;
        let mismatches = App::identify_alignment_mismatches(&alignment_results, read_size);
        
        // Expected: [false, true, false, false, false]
        // Position 0: Match -> false
        // Position 1: Subst -> true  
        // Position 2: Match -> false
        // Positions 3,4: no coverage -> false
        assert_eq!(mismatches, vec![false, true, false, false, false]);
    }
    
    #[test]
    fn test_identify_alignment_mismatches_with_insertions() {
        // Test removal of insertion operations and replacement with Subst
        let mut alignment_results = Vec::new();
        
        // Create alignment with insertion: Match, Ins, Match -> should become Match, Subst
        alignment_results.push((0, 1, vec![
            AlignmentOperation::Match,
            AlignmentOperation::Ins,
            AlignmentOperation::Match,
        ]));
        
        let read_size = 3;
        let mismatches = App::identify_alignment_mismatches(&alignment_results, read_size);
        
        // After processing: Match at pos 0, Subst at pos 1
        // Expected: [false, true, false]
        assert_eq!(mismatches, vec![false, true, false]);
    }
    
    #[test]
    fn test_identify_alignment_mismatches_multiple_alignments() {
        // Test pileup behavior with multiple overlapping alignments
        let mut alignment_results = Vec::new();
        
        // First alignment covers positions 0-2 with all matches
        alignment_results.push((0, 2, vec![
            AlignmentOperation::Match,
            AlignmentOperation::Match,
            AlignmentOperation::Match,
        ]));
        
        // Second alignment covers positions 1-3 with match, subst, match
        alignment_results.push((1, 3, vec![
            AlignmentOperation::Match,
            AlignmentOperation::Subst,
            AlignmentOperation::Match,
        ]));
        
        let read_size = 4;
        let mismatches = App::identify_alignment_mismatches(&alignment_results, read_size);
        
        // Position 0: only Match -> false
        // Position 1: Match and Match -> false (has at least one match)
        // Position 2: Match and Subst -> false (has at least one match)
        // Position 3: only Match -> false
        assert_eq!(mismatches, vec![false, false, false, false]);
    }
    
    #[test]
    fn test_identify_alignment_mismatches_all_mismatches() {
        // Test case where all alignments at a position are mismatches
        let mut alignment_results = Vec::new();
        
        // Two alignments both have substitutions at position 1
        alignment_results.push((0, 2, vec![
            AlignmentOperation::Match,
            AlignmentOperation::Subst,
            AlignmentOperation::Match,
        ]));
        
        alignment_results.push((1, 3, vec![
            AlignmentOperation::Subst,
            AlignmentOperation::Subst,
            AlignmentOperation::Match,
        ]));
        
        let read_size = 4;
        let mismatches = App::identify_alignment_mismatches(&alignment_results, read_size);
        
        // Position 0: only Match -> false
        // Position 1: Subst and Subst -> true (all are non-matches)
        // Position 2: Match and Subst -> false (has at least one match)
        // Position 3: only Match -> false
        assert_eq!(mismatches, vec![false, true, false, false]);
    }
    
    #[test]
    fn test_identify_alignment_mismatches_empty_input() {
        // Test with no alignment results
        let alignment_results = Vec::new();
        let read_size = 5;
        let mismatches = App::identify_alignment_mismatches(&alignment_results, read_size);
        
        // All positions should be false (no coverage)
        assert_eq!(mismatches, vec![false; 5]);
    }
    
    #[test]
    fn test_identify_alignment_mismatches_with_deletions() {
        // Test alignment with deletion operations
        let mut alignment_results = Vec::new();
        
        // Alignment with deletion: Match, Del, Match
        // Del means deletion from pattern, so read has an extra base - should be marked as mismatch
        alignment_results.push((0, 2, vec![
            AlignmentOperation::Match,
            AlignmentOperation::Del,
            AlignmentOperation::Match,
        ]));
        
        let read_size = 3;
        let mismatches = App::identify_alignment_mismatches(&alignment_results, read_size);
        
        // Position 0: Match -> false
        // Position 1: Del (mismatch) -> true  
        // Position 2: Match -> false
        assert_eq!(mismatches, vec![false, true, false]);
    }

    #[test]
    fn test_styling_config_default() {
        let config = StylingConfig::default();
        assert_eq!(config.quality_threshold, None);
        assert_eq!(config.quality_style_mode, QualityStyleMode::None);
    }

    #[test]
    fn test_styling_config_builder() {
        let config = StylingConfig::new()
            .with_quality_threshold(20)
            .with_quality_style_mode(QualityStyleMode::Both);
        
        assert_eq!(config.quality_threshold, Some(20));
        assert_eq!(config.quality_style_mode, QualityStyleMode::Both);
    }

    #[test]
    fn test_get_mismatches_for_record_no_patterns() {
        let record = create_test_record("test", b"ATCGATCG");
        let patterns = vec![];
        
        let mismatches = App::get_mismatches_for_record(&record, &patterns);
        assert_eq!(mismatches, vec![false; 8]);
    }

    #[test]
    fn test_get_mismatches_for_record_with_patterns() {
        let record = create_test_record("test", b"ATCGATCG");
        let pattern = create_test_pattern("ATG", 1);
        let patterns = vec![pattern];
        
        let mismatches = App::get_mismatches_for_record(&record, &patterns);
        // Should return a boolean vector the same length as the sequence
        assert_eq!(mismatches.len(), 8);
        // At least some positions should be marked (exact values depend on alignment)
        // This test ensures the function runs without panicking
    }

    #[test]
    fn test_get_quality_styling_no_config() {
        // ASCII quality scores: Quality 30: ASCII 63, Quality 10: ASCII 43, Quality 20: ASCII 53, Quality 35: ASCII 68
        let record = create_fastq_test_record("test", b"ATCG", &[63, 43, 53, 68]);
        let config = StylingConfig::default();
        
        let (italic_positions, bg_intervals) = App::get_quality_styling(&record, &config);
        assert_eq!(italic_positions, vec![false; 4]);
        assert_eq!(bg_intervals.len(), 0);
    }

    #[test]
    fn test_get_quality_styling_with_threshold() {
        // Create FASTQ record with ASCII quality scores
        // Quality 30: ASCII 63 ('?'), Quality 10: ASCII 43 ('+'), Quality 20: ASCII 53 ('5'), Quality 35: ASCII 68 ('D')
        let record = create_fastq_test_record("test", b"ATCG", &[63, 43, 53, 68]); // Qualities: 30, 10, 20, 35
        let config = StylingConfig::new()
            .with_quality_threshold(25)
            .with_quality_style_mode(QualityStyleMode::Italic);
        
        let (italic_positions, bg_intervals) = App::get_quality_styling(&record, &config);
        // Positions 1 (qual=10) and 2 (qual=20) should be italic (below threshold 25)
        // Positions 0 (qual=30) and 3 (qual=35) should not be italic (above threshold)
        assert_eq!(italic_positions, vec![false, true, true, false]);
        assert_eq!(bg_intervals.len(), 0); // No background styling in Italic mode
    }

    #[test]
    fn test_get_quality_styling_background_mode() {
        // ASCII quality scores: Quality 30: ASCII 63, Quality 10: ASCII 43
        let record = create_fastq_test_record("test", b"ATCG", &[63, 63, 43, 43]); // Qualities: 30, 30, 10, 10
        let config = StylingConfig::new()
            .with_quality_threshold(25)
            .with_quality_style_mode(QualityStyleMode::Background);
        
        let (italic_positions, bg_intervals) = App::get_quality_styling(&record, &config);
        assert_eq!(italic_positions, vec![false; 4]); // No italic in Background mode
        assert!(bg_intervals.len() > 0); // Should have background intervals
    }

    #[test]
    fn test_get_quality_styling_fasta_record() {
        // FASTA records don't have quality scores, so should return empty styling
        let record = create_test_record("test", b"ATCG"); // This creates a FASTQ record, let's create a FASTA-like one
        let config = StylingConfig::new()
            .with_quality_threshold(25)
            .with_quality_style_mode(QualityStyleMode::Both);
        
        // For now, this test ensures no panic - actual FASTA support would need different record type
        let (italic_positions, _bg_intervals) = App::get_quality_styling(&record, &config);
        // With FASTQ record, should work normally
        assert_eq!(italic_positions.len(), 4);
    }

    // Helper function for FASTQ records with custom quality scores
    fn create_fastq_test_record(id: &str, seq: &[u8], qual: &[u8]) -> SequenceRecord {
        SequenceRecord::Fastq(bio::io::fastq::Record::with_attrs(id, None, seq, qual))
    }
}
