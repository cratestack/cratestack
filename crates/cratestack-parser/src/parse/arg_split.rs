//! Top-level comma splitting for procedure argument lists.
//!
//! Split from `procedures.rs` (200-LoC ceiling). Small, but a genuinely
//! separate concern: `procedures.rs` decides what an argument *means*,
//! this decides where one *ends*.

/// Splits an argument list on top-level commas only, so a
/// parametric type's own argument list (`Geography(Polygon, 4326)`,
/// cratestack#842) stays within a single segment instead of being
/// torn in half. Segments are returned with separators removed and
/// in source order, so the caller's `segment.len() + 1` offset walk
/// is unchanged.
pub(super) fn split_top_level_commas(args_src: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in args_src.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                segments.push(&args_src[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    segments.push(&args_src[start..]);
    segments
}
