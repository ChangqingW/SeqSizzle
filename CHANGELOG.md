# Changelog

Notable changes to this project will be documented in this file.

## [0.4.1]
 * Fixed copy mode not working on startup

## [0.4.0]
 * Refined `enrich` subcommand
    * added `--max-reads` and `--sampling` options to limit the number of reads processed
    * refined homopolyer filtering and kmer assembly
    * Assembly is now optional with `--skip-assemble` flag
    * added `counts_per_read` output column
 * Defaults to start in a copy mode (new, allows mouse selection and copying text from terminal, no side borders)
 * Fixed typos in adapter presets

## [0.3.0]
 * added kmer enrichment analysis (`enrich` subcommand)

## [0.2.0]
 * added FASTA file format support
 * added gzipped file support (decompresses 10MB to a temporary file)
 * added visual styling for sequence mismatches (bold) and FASTQ quality scores (italic/color)
 * fixed shift+arrow key handling in search panel

## [0.1.5]
 * fixed build error (expected `Rect`, found `Size`)
 * updated summarize subcommand option to print percentages or counts

## [0.1.4]
 * renamed binary to `seqsizzle` to comply with Rust package naming conventions
 * published `seqsizzle` on [crates.io](https://crates.io/crates/seqsizzle)

## [0.1.3]
 * added summarize subcommand - to be moved to the UI interface later
 * better handling of polyA/T patterns 

## [0.1.2]
 * added save pattern as CSV dialog box

## [0.1.1]
 * With exact match, discard surrounding fuzzy match.  
 * added comments field

## [0.1.0] - Initial release
 * Fixed backspace cannot delete patterns in search panel
 * Added read / save search patterns with CSV file
 * Changed to use Enter to add new patterns instead of `ALT+5`
 * Display error / info messages in red at the bottom
