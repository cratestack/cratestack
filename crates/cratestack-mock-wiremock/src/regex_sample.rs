//! Builds a string that matches a `@regex` pattern by walking its parsed
//! HIR: literals verbatim, classes by a printable member, repetitions at
//! their minimum (plus `extra` extra iterations, so a caller can lengthen
//! the sample to meet a `@length` minimum), alternations by their first
//! branch. Callers must still verify the result against the real regex —
//! assertions such as `\b` or look-arounds are emitted as empty.

use regex_syntax::hir::{Class, Hir, HirKind};

const MAX_REPEAT: u32 = 1024;

pub(crate) fn sample(pattern: &str, extra: u32) -> Option<String> {
    let hir = regex_syntax::Parser::new().parse(pattern).ok()?;
    let mut out = String::new();
    walk(&hir, extra, &mut out)?;
    Some(out)
}

fn walk(hir: &Hir, extra: u32, out: &mut String) -> Option<()> {
    match hir.kind() {
        HirKind::Empty | HirKind::Look(_) => {}
        HirKind::Literal(literal) => out.push_str(std::str::from_utf8(&literal.0).ok()?),
        HirKind::Class(class) => out.push(class_member(class)?),
        HirKind::Capture(capture) => walk(&capture.sub, extra, out)?,
        HirKind::Concat(parts) => {
            for part in parts {
                walk(part, extra, out)?;
            }
        }
        HirKind::Alternation(branches) => {
            let mut last = None;
            for branch in branches {
                let mut attempt = String::new();
                last = walk(branch, extra, &mut attempt).map(|()| attempt);
                if last.is_some() {
                    break;
                }
            }
            out.push_str(&last?);
        }
        HirKind::Repetition(rep) => {
            let wanted = rep.min.saturating_add(extra);
            let count = rep
                .max
                .map_or(wanted, |max| wanted.min(max))
                .min(MAX_REPEAT);
            for _ in 0..count {
                walk(&rep.sub, extra, out)?;
            }
        }
    }
    Some(())
}

/// A deterministic, printable member of `class`: the first ASCII
/// alphanumeric, else the first ASCII graphic, else the first non-control.
fn class_member(class: &Class) -> Option<char> {
    let members: Vec<char> = match class {
        Class::Unicode(set) => set
            .iter()
            .flat_map(|r| (r.start() as u32..=r.end() as u32).take(256))
            .filter_map(char::from_u32)
            .collect(),
        Class::Bytes(set) => set
            .iter()
            .flat_map(|r| r.start()..=r.end())
            .map(char::from)
            .collect(),
    };
    members
        .iter()
        .copied()
        .find(char::is_ascii_alphanumeric)
        .or_else(|| members.iter().copied().find(char::is_ascii_graphic))
        .or_else(|| members.iter().copied().find(|c| !c.is_control()))
}
