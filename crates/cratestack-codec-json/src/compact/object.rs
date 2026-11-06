//! The object half of the rewrite: struct keys and the `Page` wrapper.

use cratestack_core::CratestackError;
use cratestack_core::compact::{KeyNode, KeyStruct};

use super::{Mode, Scan, fail};

impl Scan<'_> {
    pub(super) fn object(
        &mut self,
        node: &KeyNode,
        out: &mut Vec<u8>,
        depth: usize,
    ) -> Result<(), CratestackError> {
        let (fields, page) = match node {
            KeyNode::Struct(i) => (
                Some(
                    self.tables
                        .types
                        .get(*i as usize)
                        .ok_or_else(|| fail("the key table names a missing struct"))?,
                ),
                None,
            ),
            KeyNode::Page(inner) => (None, Some(KeyNode::List(inner.clone()))),
            KeyNode::List(_) | KeyNode::Opaque => return self.copy(out),
        };
        self.expect(b'{')?;
        out.push(b'{');
        let mut first = true;
        if self.peek()? == b'}' {
            self.pos += 1;
            out.push(b'}');
            return Ok(());
        }
        loop {
            // The entry is built aside: a dropped one leaves no comma behind.
            let mut entry = Vec::new();
            if self.entry(fields, page.as_ref(), &mut entry, depth)? {
                if !first {
                    out.push(b',');
                }
                first = false;
                out.extend_from_slice(&entry);
            }
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    out.push(b'}');
                    return Ok(());
                }
                _ => return Err(fail("expected `,` or `}`")),
            }
        }
    }

    /// One `"key": value`. `false` when the entry was dropped.
    fn entry(
        &mut self,
        fields: Option<&KeyStruct>,
        items: Option<&KeyNode>,
        entry: &mut Vec<u8>,
        depth: usize,
    ) -> Result<bool, CratestackError> {
        if self.peek()? != b'"' {
            return Err(fail("expected a string key"));
        }
        let (key, span) = self.string()?;
        self.expect(b':')?;
        let Some(fields) = fields else {
            // Page: the wrapper's keys are named; `items` holds the list.
            entry.extend_from_slice(&self.src[span]);
            entry.push(b':');
            match items {
                Some(list) if key == "items" => self.value(list, entry, depth + 1)?,
                _ => self.copy(entry)?,
            }
            return Ok(true);
        };
        let by_name = fields.fields.iter().position(|(name, _)| *name == key);
        let (index, rewritten) = match (self.mode, by_name) {
            (Mode::ToCompact, Some(i)) => (Some(i), format!("\"{i}\"")),
            (Mode::ToCompact, None) => {
                // The peer's table has no such field: it could not read it.
                self.copy(&mut Vec::new())?;
                return Ok(false);
            }
            (Mode::FromCompact, Some(i)) => (
                Some(i),
                String::from_utf8_lossy(&self.src[span]).into_owned(),
            ),
            (Mode::FromCompact, None) if is_number(&key) => {
                let i = key
                    .parse::<usize>()
                    .ok()
                    .filter(|&i| i < fields.fields.len() && key == i.to_string())
                    .ok_or_else(|| fail(format!("unknown compact key {key}")))?;
                (
                    Some(i),
                    serde_json::to_string(&fields.fields[i].0).map_err(fail)?,
                )
            }
            (Mode::FromCompact, None) => {
                (None, String::from_utf8_lossy(&self.src[span]).into_owned())
            }
        };
        entry.extend_from_slice(rewritten.as_bytes());
        entry.push(b':');
        match index {
            Some(i) => self.value(&fields.fields[i].1, entry, depth + 1)?,
            None => self.copy(entry)?,
        }
        Ok(true)
    }
}

/// Looks like a compact key (decimal digits). A field name cannot start
/// with a digit, so such a key is never a name; one that is not the
/// canonical spelling of an index in range is an error, not a pass-through.
fn is_number(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit())
}
