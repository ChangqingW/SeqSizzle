mod interval_operations;
pub mod match_highlighting;
pub mod combined_styling;

pub use crate::read_stylizing::match_highlighting::{highlight_matches, format_overlap};
pub use crate::read_stylizing::combined_styling::{
    CombinedStyle, StyleInput, highlight_with_combined_styles, 
    bool_vector_to_intervals, quality_to_bg_color
};
