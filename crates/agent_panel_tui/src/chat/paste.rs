//! Paste handling for the chat input box.
//!
//! Long pastes (multi-line, or a single very long line) are
//! collapsed into a `[Pasted ~N lines]` placeholder in the input
//! display so the prompt stays compact. The full content is held
//! alongside the placeholder and re-substituted at submit time via
//! [`ChatPanelState::take_full_input_text`](super::state::ChatPanelState::take_full_input_text).
//!
//! The host's event loop calls
//! [`ChatPanelState::push_paste`](super::state::ChatPanelState::push_paste)
//! for each `Event::Paste`. Short pastes are inserted verbatim;
//! long pastes go through the placeholder/expand path.
//!
//! The two thresholds match the values used by opencode's CLI TUI
//! — they were picked empirically to balance "I can see what I
//! pasted" against "the input box shouldn't be a 5000-char wall".

/// Lower bound (in lines, including 1) above which a paste is collapsed
/// into a placeholder. Mirrors the heuristic used by opencode's CLI
/// TUI to keep long pastes from drowning the chat panel.
pub const PASTE_SUMMARY_LINE_THRESHOLD: usize = 3;

/// Lower bound (in characters) above which a single-line paste is
/// also collapsed. The placeholder still works for one very long
/// line.
pub const PASTE_SUMMARY_LEN_THRESHOLD: usize = 150;

/// A paste event recorded in the chat panel's input buffer.
///
/// `placeholder` is the short text shown in the input box (and in
/// the chat history). `content` is the full original text. For
/// short pastes, `placeholder == content`; for long pastes,
/// `placeholder` is a `[Pasted ~N lines]` marker.
///
/// The chat panel stores a `Vec<PasteEntry>` parallel to the
/// display string in `input: String`. `take_full_input_text`
/// walks the display string and re-substitutes each placeholder
/// with the corresponding content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteEntry {
    pub placeholder: String,
    pub content: String,
}

impl PasteEntry {
    /// Build a paste entry from a content string, returning the
    /// `(display, entry)` pair the chat panel needs. Short
    /// pastes are returned with `display == content`; long
    /// pastes are returned with a `[Pasted ~N lines]`
    /// placeholder.
    pub fn from_content(content: &str) -> Self {
        if let Some(placeholder) = summarize_paste(content) {
            Self {
                placeholder,
                content: content.to_string(),
            }
        } else {
            // Short paste: the display IS the content.
            Self {
                placeholder: content.to_string(),
                content: content.to_string(),
            }
        }
    }
}

/// Decide whether a paste should be collapsed into a compact
/// placeholder. Returns `true` when the content crosses either
/// the line-count or the char-count threshold.
pub fn needs_placeholder(content: &str) -> bool {
    let line_count = content.lines().count().max(1);
    line_count >= PASTE_SUMMARY_LINE_THRESHOLD
        || content.chars().count() > PASTE_SUMMARY_LEN_THRESHOLD
}

/// Build a `[Pasted ~N lines]` placeholder for the given content.
/// Returns `None` for short pastes that should be inserted
/// verbatim.
///
/// The exact line count of the original is used (not the trimmed
/// content) so the placeholder matches what the user sees in their
/// clipboard.
pub fn summarize_paste(content: &str) -> Option<String> {
    if needs_placeholder(content) {
        let line_count = content.lines().count().max(1);
        Some(format!("[Pasted ~{line_count} lines]"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        needs_placeholder, summarize_paste, PasteEntry, PASTE_SUMMARY_LEN_THRESHOLD,
        PASTE_SUMMARY_LINE_THRESHOLD,
    };

    #[test]
    fn short_single_line_paste_is_verbatim() {
        assert!(summarize_paste("hello world").is_none());
        assert!(!needs_placeholder("hello world"));
    }

    #[test]
    fn exactly_two_lines_is_verbatim() {
        // Threshold is 3 lines, so 2 must NOT be collapsed.
        assert!(summarize_paste("line one\nline two").is_none());
        assert!(!needs_placeholder("line one\nline two"));
    }

    #[test]
    fn three_lines_triggers_placeholder() {
        let p = summarize_paste("a\nb\nc").unwrap();
        assert_eq!(p, "[Pasted ~3 lines]");
        assert!(needs_placeholder("a\nb\nc"));
    }

    #[test]
    fn many_lines_triggers_placeholder_with_correct_count() {
        let text = (1..=10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let p = summarize_paste(&text).unwrap();
        assert_eq!(p, "[Pasted ~10 lines]");
    }

    #[test]
    fn single_very_long_line_triggers_placeholder() {
        let long = "a".repeat(PASTE_SUMMARY_LEN_THRESHOLD + 1);
        let p = summarize_paste(&long).unwrap();
        assert_eq!(p, "[Pasted ~1 lines]");
        assert!(needs_placeholder(&long));
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

    #[test]
    fn paste_entry_from_short_content_has_placeholder_equal_to_content() {
        let e = PasteEntry::from_content("hello world");
        assert_eq!(e.placeholder, "hello world");
        assert_eq!(e.content, "hello world");
    }

    #[test]
    fn paste_entry_from_long_content_has_summarized_placeholder() {
        let e = PasteEntry::from_content("a\nb\nc\nd");
        assert_eq!(e.placeholder, "[Pasted ~4 lines]");
        assert_eq!(e.content, "a\nb\nc\nd");
    }

    #[test]
    fn needs_placeholder_matches_summarize_paste() {
        // The two functions must agree, since the chat panel uses
        // one to gate the other.
        for sample in ["", "short", "a\nb", "a\nb\nc", &"x".repeat(200)] {
            assert_eq!(
                needs_placeholder(sample),
                summarize_paste(sample).is_some(),
                "mismatch on {sample:?}",
            );
        }
    }
}
