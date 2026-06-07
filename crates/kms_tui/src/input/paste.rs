/// Lower bound (in lines, including 1) above which a paste is collapsed
/// into a placeholder. Mirrors the heuristic used by opencode's CLI
/// TUI to keep long pastes from drowning the chat panel.
pub const PASTE_SUMMARY_LINE_THRESHOLD: usize = 3;
/// Lower bound (in characters) above which a single-line paste is also
/// collapsed. The placeholder still works for one very long line.
pub const PASTE_SUMMARY_LEN_THRESHOLD: usize = 150;

/// Decide whether a paste should be collapsed into a compact
/// placeholder (`[Pasted ~N lines]`) and, if so, build that
/// placeholder. Returns `None` for short pastes that should be
/// inserted verbatim.
///
/// The exact line count of the original is used (not the trimmed
/// content) so the placeholder matches what the user sees in their
/// clipboard.
pub fn summarize_paste(content: &str) -> Option<String> {
    let line_count = content.lines().count().max(1);
    let needs_summary = line_count >= PASTE_SUMMARY_LINE_THRESHOLD
        || content.chars().count() > PASTE_SUMMARY_LEN_THRESHOLD;
    if needs_summary {
        Some(format!("[Pasted ~{line_count} lines]"))
    } else {
        None
    }
}

#[cfg(test)]
mod paste_summary_tests {
    use super::{summarize_paste, PASTE_SUMMARY_LEN_THRESHOLD, PASTE_SUMMARY_LINE_THRESHOLD};

    #[test]
    fn short_single_line_paste_is_verbatim() {
        assert!(summarize_paste("hello world").is_none());
    }

    #[test]
    fn exactly_two_lines_is_verbatim() {
        // Threshold is 3 lines, so 2 must NOT be collapsed.
        assert!(summarize_paste("line one\nline two").is_none());
    }

    #[test]
    fn three_lines_triggers_placeholder() {
        let p = summarize_paste("a\nb\nc").unwrap();
        assert_eq!(p, "[Pasted ~3 lines]");
    }

    #[test]
    fn many_lines_triggers_placeholder_with_correct_count() {
        let text = (1..=10).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let p = summarize_paste(&text).unwrap();
        assert_eq!(p, "[Pasted ~10 lines]");
    }

    #[test]
    fn single_very_long_line_triggers_placeholder() {
        let long = "a".repeat(PASTE_SUMMARY_LEN_THRESHOLD + 1);
        let p = summarize_paste(&long).unwrap();
        assert_eq!(p, "[Pasted ~1 lines]");
    }

    #[test]
    fn blank_lines_count_toward_total() {
        // "a\n\nb" has 3 lines (one empty in the middle).
        let p = summarize_paste("a\n\nb").unwrap();
        assert_eq!(p, "[Pasted ~3 lines]");
    }

    #[test]
    fn threshold_lengths_match_opencode_default() {
        // Sanity: both thresholds match the opencode CLI values used
        // as the reference for this design.
        assert_eq!(PASTE_SUMMARY_LINE_THRESHOLD, 3);
        assert_eq!(PASTE_SUMMARY_LEN_THRESHOLD, 150);
    }
}
