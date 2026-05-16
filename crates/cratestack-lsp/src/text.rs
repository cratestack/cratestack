use cratestack_core::SourceSpan;
use tower_lsp_server::ls_types::{Position, Range};

pub(crate) fn span_contains(span: SourceSpan, offset: usize) -> bool {
    span.start <= offset && offset <= span.end
}

pub(crate) fn span_to_range(text: &str, span: SourceSpan) -> Option<Range> {
    Some(range_from_offsets(text, span.start, span.end))
}

pub(crate) fn range_from_offsets(text: &str, start: usize, end: usize) -> Range {
    Range {
        start: offset_to_position(text, start),
        end: offset_to_position(text, end),
    }
}

pub(crate) fn position_to_offset(text: &str, position: Position) -> Option<usize> {
    let line_index = position.line as usize;
    let character = position.character as usize;
    let starts = line_start_offsets(text);
    let line_start = *starts.get(line_index)?;
    let line_end = starts.get(line_index + 1).copied().unwrap_or(text.len());
    let line = &text[line_start..line_end];

    let mut offset = line_start;
    let mut utf16 = 0usize;
    for ch in line.chars() {
        if utf16 == character {
            return Some(offset);
        }
        utf16 += ch.len_utf16();
        offset += ch.len_utf8();
        if utf16 > character {
            return None;
        }
    }

    (utf16 == character).then_some(offset)
}

pub(crate) fn offset_to_position(text: &str, target: usize) -> Position {
    let bounded = target.min(text.len());
    let mut line = 0u32;
    let mut character = 0u32;

    for (offset, ch) in text.char_indices() {
        if offset >= bounded {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    Position { line, character }
}

pub(crate) fn line_start_offsets(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (offset, ch) in text.char_indices() {
        if ch == '\n' {
            starts.push(offset + ch.len_utf8());
        }
    }
    starts
}

pub(crate) fn word_at_offset(text: &str, offset: usize) -> Option<&str> {
    if text.is_empty() {
        return None;
    }
    let bytes = text.as_bytes();
    let mut start = offset.min(bytes.len().saturating_sub(1));
    if !is_word_byte(*bytes.get(start)?) {
        if start > 0 && is_word_byte(bytes[start - 1]) {
            start -= 1;
        } else {
            return None;
        }
    }
    let mut end = start;
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    while end + 1 < bytes.len() && is_word_byte(bytes[end + 1]) {
        end += 1;
    }
    text.get(start..=end)
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
