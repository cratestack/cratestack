use cratestack_core::SourceSpan;

use crate::state::SpannedName;

/// Mixin names mentioned by `@use(...)` directives, recovered from source text.
///
/// This has to read the document rather than the IR: `expand_model_mixins`
/// (cratestack-parser) inlines each mixin's fields into the model and then
/// *drops* the `@use(...)` attribute from `Model::attributes`, so by the time a
/// `Schema` reaches the language server the reference site no longer exists in
/// it. `analyze.rs` recovers `@relation(...)` spans from text for the same
/// reason.
///
/// Handles the multi-name form (`@use(Timestamps, SoftDelete)`) that
/// `parse_model_use_attribute` accepts.
pub(crate) fn mixin_use_names(text: &str) -> Vec<SpannedName> {
    const PREFIX: &str = "@use(";

    let mut names = Vec::new();
    let mut line_start = 0usize;
    for (line_number, line) in text.split('\n').enumerate() {
        let Some(open) = line.find(PREFIX) else {
            line_start += line.len() + 1;
            continue;
        };
        // Only a directive when `@use(` opens the line — `@use(` inside a
        // comment or a string is not a reference site.
        if !line.trim_start().starts_with(PREFIX) {
            line_start += line.len() + 1;
            continue;
        }
        if let Some(close) = line[open..].find(')').map(|index| open + index) {
            let inner_start = open + PREFIX.len();
            names.extend(split_names(
                &line[inner_start..close],
                line_start + inner_start,
                line_number + 1,
            ));
        }
        line_start += line.len() + 1;
    }
    names
}

fn split_names(inner: &str, absolute_start: usize, line: usize) -> Vec<SpannedName> {
    let mut names = Vec::new();
    let mut cursor = 0usize;
    for part in inner.split(',') {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            let lead = part.find(trimmed).unwrap_or_default();
            let start = absolute_start + cursor + lead;
            names.push(SpannedName {
                name: trimmed.to_owned(),
                span: SourceSpan {
                    start,
                    end: start + trimmed.len(),
                    line,
                },
            });
        }
        cursor += part.len() + 1;
    }
    names
}
