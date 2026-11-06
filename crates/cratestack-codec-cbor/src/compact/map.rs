//! The map half of the rewrite: struct keys and the `Page` wrapper.

use cratestack_core::CratestackError;
use cratestack_core::compact::{KeyNode, KeyStruct};
use minicbor::Decoder;
use minicbor::data::Type;

use super::{Mode, Walk, at_break, fail};

/// What a map's keys mean.
enum Keys<'a> {
    /// Field names and integers are interchangeable per the table.
    Struct(&'a KeyStruct),
    /// `Page`: the wrapper's keys stay named; `items` holds the list.
    Page(&'a KeyNode),
}

impl Walk<'_> {
    pub(super) fn map(
        &mut self,
        d: &mut Decoder<'_>,
        node: &KeyNode,
        out: &mut Vec<u8>,
        depth: usize,
    ) -> Result<(), CratestackError> {
        let keys = match node {
            KeyNode::Struct(i) => Keys::Struct(
                self.tables
                    .types
                    .get(*i as usize)
                    .ok_or_else(|| fail("the key table names a missing struct"))?,
            ),
            KeyNode::Page(inner) => Keys::Page(inner),
            KeyNode::List(_) | KeyNode::Opaque => return self.copy(d, out),
        };
        let len = d.map().map_err(fail)?;
        // Entries are built aside: dropping one changes the head's count.
        let mut body = Vec::new();
        let mut kept = 0_u64;
        let mut seen = 0_u64;
        while len.is_none_or(|n| seen < n) {
            if len.is_none() && at_break(d)? {
                break;
            }
            seen += 1;
            if self.entry(d, &keys, &mut body, depth)? {
                kept += 1;
            }
        }
        let mut head = minicbor::Encoder::new(&mut *out);
        match len {
            Some(_) => head.map(kept).map(drop),
            None => head.begin_map().map(drop),
        }
        .map_err(fail)?;
        out.extend_from_slice(&body);
        if len.is_none() {
            out.push(0xff);
        }
        Ok(())
    }

    /// One key and value. `false` when the entry was dropped.
    fn entry(
        &mut self,
        d: &mut Decoder<'_>,
        keys: &Keys<'_>,
        body: &mut Vec<u8>,
        depth: usize,
    ) -> Result<bool, CratestackError> {
        match d.datatype().map_err(fail)? {
            Type::String => {
                let name = d.str().map_err(fail)?;
                self.named_entry(d, keys, name, body, depth)
            }
            Type::U8 | Type::U16 | Type::U32 | Type::U64 if self.mode == Mode::FromCompact => {
                let Keys::Struct(fields) = keys else {
                    return Err(fail("an integer key where the wrapper keys are named"));
                };
                let index = d.u64().map_err(fail)?;
                let (name, node) = usize::try_from(index)
                    .ok()
                    .and_then(|i| fields.fields.get(i))
                    .ok_or_else(|| fail(format!("unknown compact key {index}")))?;
                minicbor::Encoder::new(&mut *body).str(name).map_err(fail)?;
                self.value(d, node, body, depth + 1)?;
                Ok(true)
            }
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::Int
                if self.mode == Mode::FromCompact && matches!(keys, Keys::Struct(_)) =>
            {
                Err(fail("negative compact key"))
            }
            // Any other key (an integer on the way out, a bytes key, ...) is
            // not ours to interpret: it and its value are copied.
            _ => {
                self.copy(d, body)?;
                self.copy(d, body)?;
                Ok(true)
            }
        }
    }

    fn named_entry(
        &mut self,
        d: &mut Decoder<'_>,
        keys: &Keys<'_>,
        name: &str,
        body: &mut Vec<u8>,
        depth: usize,
    ) -> Result<bool, CratestackError> {
        let name = name.to_owned();
        let write_key = |body: &mut Vec<u8>| -> Result<(), CratestackError> {
            minicbor::Encoder::new(&mut *body)
                .str(&name)
                .map(drop)
                .map_err(fail)
        };
        match keys {
            Keys::Page(inner) => {
                write_key(body)?;
                if name == "items" {
                    let items = KeyNode::List(Box::new((*inner).clone()));
                    self.value(d, &items, body, depth + 1)?;
                } else {
                    self.copy(d, body)?;
                }
                Ok(true)
            }
            Keys::Struct(fields) => {
                let found = fields.fields.iter().position(|(field, _)| *field == name);
                match (self.mode, found) {
                    (Mode::ToCompact, Some(i)) => {
                        minicbor::Encoder::new(&mut *body)
                            .u64(i as u64)
                            .map_err(fail)?;
                        self.value(d, &fields.fields[i].1, body, depth + 1)?;
                        Ok(true)
                    }
                    // The peer's table has no such field: it could not read it.
                    (Mode::ToCompact, None) => {
                        d.skip().map_err(fail)?;
                        Ok(false)
                    }
                    // A text key is accepted as it is; a known field's value
                    // still gets its own nested keys rewritten.
                    (Mode::FromCompact, Some(i)) => {
                        write_key(body)?;
                        self.value(d, &fields.fields[i].1, body, depth + 1)?;
                        Ok(true)
                    }
                    (Mode::FromCompact, None) => {
                        write_key(body)?;
                        self.copy(d, body)?;
                        Ok(true)
                    }
                }
            }
        }
    }
}
