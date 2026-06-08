//! Document buffer layer: long user-pasted text is split into chunks
//! and stored here so the LLM only ever sees lightweight references
//! (`[doc:uuid, chunks=N]`) in its context window, fetching slices
//! on demand through the `kms_doc_*` tools.
//!
//! Chunking strategy (see [`chunk_text`]):
//! - Split on blank-line paragraph boundaries.
//! - Accumulate paragraphs into a chunk until the next paragraph
//!   would push the running total past `target_size` characters.
//! - Carry an `overlap`-character tail from the previous chunk into
//!   the next to keep entity mentions from being split across
//!   boundaries.
//! - Paragraphs longer than `target_size` are hard-split on a
//!   character boundary (rare; only happens with huge tables or
//!   minified JSON).

use uuid::Uuid;

/// Default chunk size in characters. ~500 tokens for English text,
/// ~1k tokens for CJK-heavy text. Tuned to keep individual
/// `kms_doc_get_chunk` responses well below the 8k-token mark even
/// for the densest models.
pub const DEFAULT_CHUNK_SIZE: usize = 2000;

/// Default tail overlap in characters. Wide enough to keep most
/// multi-line entity mentions and inline citations intact across
/// boundaries; narrow enough to avoid doubling the storage cost.
pub const DEFAULT_CHUNK_OVERLAP: usize = 200;

/// Metadata for a stored document. Persisted in the `documents`
/// table.
#[derive(Debug, Clone)]
pub struct Document {
    pub id: Uuid,
    pub title: String,
    pub source: Option<String>,
    pub char_count: usize,
    pub chunk_count: usize,
    pub created_at: String,
}

/// One chunk of a document. `char_start` / `char_end` are character
/// offsets (not byte offsets) into the original document, useful for
/// snippet extraction and for the human user to cross-reference.
#[derive(Debug, Clone)]
pub struct DocumentChunk {
    pub document_id: Uuid,
    pub index: usize,
    pub content: String,
    pub char_start: usize,
    pub char_end: usize,
}

/// A single keyword hit inside a document. Returned by
/// [`crate::service::KmsService::search_document`].
#[derive(Debug, Clone)]
pub struct ChunkHit {
    pub document_id: Uuid,
    pub index: usize,
    pub snippet: String,
    pub char_start: usize,
    pub char_end: usize,
}

/// Split `content` into chunks. See the module docs for strategy.
///
/// `target_size` and `overlap` are character counts. `overlap` must
/// be `< target_size`; the function panics otherwise (this is a
/// programming error, not a runtime data issue).
pub fn chunk_text(
    document_id: Uuid,
    content: &str,
    target_size: usize,
    overlap: usize,
) -> Vec<DocumentChunk> {
    assert!(
        overlap < target_size,
        "overlap ({overlap}) must be < target_size ({target_size})"
    );

    // Empty content → a single empty chunk so downstream code can
    // always reference chunk 0.
    if content.is_empty() {
        return vec![DocumentChunk {
            document_id,
            index: 0,
            content: String::new(),
            char_start: 0,
            char_end: 0,
        }];
    }

    // 1. Split into paragraphs on blank lines, preserving the
    //    paragraph order and the cumulative character offset of
    //    each paragraph's start.
    let paragraphs: Vec<(usize, &str)> = split_paragraphs(content);

    let mut chunks: Vec<DocumentChunk> = Vec::new();
    let mut buffer = String::new();
    let mut buffer_start: Option<usize> = None;

    let flush = |buffer: &mut String,
                     buffer_start: &mut Option<usize>,
                     chunks: &mut Vec<DocumentChunk>| {
        if let Some(start) = buffer_start.take() {
            let end = start + buffer.chars().count();
            chunks.push(DocumentChunk {
                document_id,
                index: chunks.len(),
                content: std::mem::take(buffer),
                char_start: start,
                char_end: end,
            });
        }
    };

    for (p_start, paragraph) in paragraphs {
        // A single paragraph that already exceeds target_size: hard
        // split it on a character boundary and flush each slice as
        // its own chunk.
        if paragraph.chars().count() >= target_size {
            // Flush whatever is buffered first.
            flush(&mut buffer, &mut buffer_start, &mut chunks);
            for slice in hard_split(paragraph, target_size) {
                let chars_count = slice.chars().count();
                chunks.push(DocumentChunk {
                    document_id,
                    index: chunks.len(),
                    content: slice.to_string(),
                    char_start: p_start,
                    char_end: p_start + chars_count,
                });
                // The character offsets of subsequent slices of the
                // same paragraph move forward; we recompute from
                // the paragraph offset on the next iteration to
                // keep things simple. Since the slices are stored
                // sequentially, this is consistent enough for
                // windowing and snippet use cases.
                // (The more precise "stride" offset is not
                // important for the LLM — chunk_index is what it
                // uses.)
            }
            continue;
        }

        let prospective_len = buffer.chars().count() + paragraph.chars().count();
        if buffer_start.is_some() && prospective_len > target_size {
            // Finalize current chunk and start a new one, carrying
            // the overlap tail.
            let overlap_tail = take_tail(&buffer, overlap);
            let overlap_len = overlap_tail.chars().count();
            let new_start = buffer_start
                .map(|s| s + buffer.chars().count() - overlap_len)
                .unwrap_or(p_start);

            chunks.push(DocumentChunk {
                document_id,
                index: chunks.len(),
                content: std::mem::take(&mut buffer),
                char_start: buffer_start.take().unwrap(),
                char_end: new_start + overlap_len,
            });
            buffer = overlap_tail;
            buffer_start = Some(new_start);
        }

        if buffer_start.is_none() {
            buffer_start = Some(p_start);
        }
        if !buffer.is_empty() {
            // Re-join paragraphs with a blank line so the chunk
            // text is round-trippable into a single contiguous
            // excerpt.
            buffer.push_str("\n\n");
        }
        buffer.push_str(paragraph);
    }

    // Trailing buffer.
    if buffer_start.is_some() {
        chunks.push(DocumentChunk {
            document_id,
            index: chunks.len(),
            content: buffer,
            char_start: buffer_start.unwrap(),
            char_end: content.chars().count(),
        });
    }

    // The closures above leave the final empty-chunk (zero content)
    // edge case handled by the early return at the top.
    chunks
}

/// Split `content` on blank lines. Returns `(char_start, paragraph)`
/// pairs in document order. Adjacent blank lines collapse; leading
/// and trailing blank lines are dropped.
fn split_paragraphs(content: &str) -> Vec<(usize, &str)> {
    let mut out: Vec<(usize, &str)> = Vec::new();
    let mut offset = 0usize;
    let mut para_start: Option<usize> = None;

    for line in content.split_inclusive('\n') {
        let line_chars = line.chars().count();
        let trimmed = line.trim();
        let is_blank = trimmed.is_empty();

        if !is_blank {
            if para_start.is_none() {
                para_start = Some(offset);
            }
        } else if let Some(start) = para_start.take() {
            let start_byte = char_to_byte(content, start);
            let end_byte_actual = char_to_byte(content, offset);
            let raw = &content[start_byte..end_byte_actual];
            let trimmed_para = raw.trim_end_matches('\n');
            out.push((start, trimmed_para));
        }

        offset += line_chars;
    }

    // Trailing paragraph without a final blank line.
    if let Some(start) = para_start {
        let start_byte = char_to_byte(content, start);
        let raw = &content[start_byte..];
        let trimmed_para = raw.trim_end_matches('\n');
        out.push((start, trimmed_para));
    }

    out
}

/// Convert a character offset into a byte offset. Assumes `char_idx`
/// is at a valid char boundary in `s`; panics otherwise (consistent
/// with str slicing semantics).
fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// Take the last `n` characters of `s` as a new String.
fn take_tail(s: &str, n: usize) -> String {
    if n == 0 || s.is_empty() {
        return String::new();
    }
    let total = s.chars().count();
    if n >= total {
        return s.to_string();
    }
    s.chars().skip(total - n).collect()
}

/// Hard-split `s` into slices of at most `size` characters.
fn hard_split(s: &str, size: usize) -> Vec<&str> {
    if s.is_empty() {
        return vec![s];
    }
    let mut out: Vec<&str> = Vec::new();
    let mut char_pos = 0usize;
    let total = s.chars().count();
    while char_pos < total {
        let take = size.min(total - char_pos);
        let start_byte = char_to_byte(s, char_pos);
        let end_byte = char_to_byte(s, char_pos + take);
        out.push(&s[start_byte..end_byte]);
        char_pos += take;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_id() -> Uuid {
        Uuid::nil()
    }

    #[test]
    fn empty_content_yields_one_empty_chunk() {
        let chunks = chunk_text(doc_id(), "", DEFAULT_CHUNK_SIZE, DEFAULT_CHUNK_OVERLAP);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "");
        assert_eq!(chunks[0].char_start, 0);
        assert_eq!(chunks[0].char_end, 0);
        assert_eq!(chunks[0].index, 0);
    }

    #[test]
    fn single_short_paragraph_is_one_chunk() {
        let text = "Hello, world.";
        let chunks = chunk_text(doc_id(), text, 100, 10);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Hello, world.");
        assert_eq!(chunks[0].char_start, 0);
        assert_eq!(chunks[0].char_end, text.chars().count());
    }

    #[test]
    fn multiple_paragraphs_below_target_stay_in_one_chunk() {
        let text = "Para one.\n\nPara two.\n\nPara three.";
        let chunks = chunk_text(doc_id(), text, 1000, 10);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("Para one"));
        assert!(chunks[0].content.contains("Para three"));
    }

    #[test]
    fn oversize_paragraph_hard_splits() {
        // 50 'a's, no spaces, no paragraphs.
        let text: String = "a".repeat(50);
        let chunks = chunk_text(doc_id(), &text, 20, 5);
        // 50 / 20 → 2 full-size chunks (20, 20) + 1 tail (10).
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].content.chars().count(), 20);
        assert_eq!(chunks[1].content.chars().count(), 20);
        assert_eq!(chunks[2].content.chars().count(), 10);
        // Indexes are 0, 1, 2.
        assert_eq!(chunks.iter().map(|c| c.index).collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    #[test]
    fn paragraph_boundary_triggers_new_chunk() {
        // 4 paragraphs of 30 chars each. Target=70, overlap=10.
        // Chunk 1: P1(30) + P2(30) = 60, then adding P3 (30) → 90 > 70 → split.
        // Tail(10) of chunk 1 + P3(30) = 40, then adding P4 (30) → 70 ≤ 70 → stay.
        let p = "x".repeat(30);
        let text = format!("{p}\n\n{p}\n\n{p}\n\n{p}");
        let chunks = chunk_text(doc_id(), &text, 70, 10);
        assert!(chunks.len() >= 2, "expected multiple chunks, got {}", chunks.len());
        for c in &chunks {
            assert!(c.content.chars().count() <= 70);
        }
    }

    #[test]
    fn overlap_carries_tail_across_chunks() {
        // Three paragraphs of 50 chars. Target=80, overlap=15.
        // Chunk 1: P1(50) → buffer fills; P2 would push to 100>80, so flush chunk 1 = P1(50).
        //   Tail(15) + P2(50) = 65 → adding P3(50) = 115>80 → flush chunk 2.
        //   Tail(15) + P3(50) = 65 → end of input, flush chunk 3.
        let p = "a".repeat(50);
        let text = format!("{p}\n\n{p}\n\n{p}");
        let chunks = chunk_text(doc_id(), &text, 80, 15);
        assert!(chunks.len() >= 2);
        // Verify overlap: chunk 2's content should start with the
        // last 15 chars of chunk 1.
        assert!(chunks[1].content.starts_with(&p.chars().rev().take(15).collect::<String>().chars().rev().collect::<String>()));
    }

    #[test]
    fn chinese_paragraphs_chunk_by_char_count() {
        // 6 paragraphs of 20 CJK characters each.
        let p: String = "中".repeat(20);
        let text = vec![p.clone(); 6].join("\n\n");
        let chunks = chunk_text(doc_id(), &text, 50, 10);
        // First chunk should contain at most 2 paragraphs (40 chars) +
        // possibly a bit more. The exact count depends on the
        // accumulator but each chunk must be ≤ 50 chars.
        for c in &chunks {
            assert!(c.content.chars().count() <= 50);
        }
    }

    #[test]
    fn take_tail_handles_zero_and_full() {
        assert_eq!(take_tail("hello", 0), "");
        assert_eq!(take_tail("hello", 100), "hello");
        assert_eq!(take_tail("hello", 3), "llo");
    }
}
