//! Key tables from an op contract: the closure's structs, interned.

use std::collections::BTreeMap;

use serde_json::Value;

use super::derive_ops;
use super::error::CompactError;
use super::tables::{KeyNode, KeyStruct, KeyTables, OpKeys};

pub(super) fn bad(what: impl Into<String>) -> CompactError {
    CompactError::Contract(what.into())
}

pub(super) fn str_of<'a>(value: &'a Value, key: &str) -> Result<&'a str, CompactError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("missing string `{key}`")))
}

pub(super) fn array_of<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], CompactError> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| bad(format!("missing array `{key}`")))
}

/// The closure's models, types and views, in contract order (each list is
/// sorted by name), with the index every name gets.
pub(super) struct Closure<'a> {
    pub(super) decls: Vec<&'a Value>,
    pub(super) names: BTreeMap<&'a str, u32>,
}

impl<'a> Closure<'a> {
    pub(super) fn read(contract: &'a Value) -> Result<Self, CompactError> {
        let closure = contract
            .get("closure")
            .ok_or_else(|| bad("missing `closure`"))?;
        let mut decls = Vec::new();
        for kind in ["models", "types", "views"] {
            // A contract written before views existed has no `views` list.
            let list = closure.get(kind).and_then(Value::as_array);
            decls.extend(list.into_iter().flatten());
        }
        let mut names = BTreeMap::new();
        for (index, decl) in decls.iter().enumerate() {
            names.insert(str_of(decl, "name")?, index as u32);
        }
        Ok(Self { decls, names })
    }

    /// The decl a node named `name` is, if the closure holds one.
    pub(super) fn decl(&self, name: &str) -> Option<&'a Value> {
        self.names
            .get(name)
            .map(|&index| self.decls[index as usize])
    }

    /// One struct per decl, numbered locally (the first decl is `Struct(0)`).
    fn structs(&self) -> Result<Vec<KeyStruct>, CompactError> {
        self.decls
            .iter()
            .map(|decl| {
                let fields = array_of(decl, "fields")?;
                let fields = fields.iter().map(|field| self.field(field));
                Ok(KeyStruct {
                    fields: fields.collect::<Result<_, _>>()?,
                })
            })
            .collect()
    }

    fn field(&self, field: &Value) -> Result<(String, KeyNode), CompactError> {
        let ty = field.get("ty").ok_or_else(|| bad("field without `ty`"))?;
        Ok((str_of(field, "name")?.to_owned(), self.node(ty)))
    }

    /// The node of a type ref, arity included.
    pub(super) fn node(&self, ty: &Value) -> KeyNode {
        let base = self.element(ty);
        match ty.get("arity").and_then(Value::as_str) {
            Some("list") => KeyNode::list(base),
            _ => base,
        }
    }

    /// The node of one element of a type ref: its arity ignored.
    /// `Page<T>` keeps its wrapper named; `FindMany<T>` and `PageInput` are
    /// framework-generated input frames, so they stay opaque like anything
    /// else the closure does not declare (scalars, enums, `Json`).
    pub(super) fn element(&self, ty: &Value) -> KeyNode {
        let name = ty.get("name").and_then(Value::as_str).unwrap_or_default();
        if name == "Page" {
            let item = ty
                .get("generic_args")
                .and_then(Value::as_array)
                .and_then(|args| args.first());
            return item.map_or(KeyNode::Opaque, |item| KeyNode::page(self.node(item)));
        }
        if matches!(
            name,
            "FindMany" | "PageInput" | "CursorPageInput" | "CursorPage"
        ) {
            return KeyNode::Opaque;
        }
        self.names
            .get(name)
            .map_or(KeyNode::Opaque, |&index| KeyNode::Struct(index))
    }
}

impl KeyTables {
    /// Adds the tables of one op's canonical contract JSON (the form
    /// [`op_contract_json`](crate::op_contract_json) prints and `ContractLock`
    /// stores) under `digest`. The op key comes from the contract itself.
    /// Adding the same `(op, digest)` again replaces the entry.
    ///
    /// The digest is taken on trust, as the lock's own entries are checked by
    /// whoever reads the lock (`ContractLock::parse` recomputes them).
    pub fn add_contract(&mut self, digest: [u8; 32], contract: &Value) -> Result<(), CompactError> {
        let op = str_of(contract, "key")?.to_owned();
        let closure = Closure::read(contract)?;
        let base = self.intern_run(closure.structs()?);
        let (request, response) = derive_ops::roots(self, contract, &closure, base)?;
        self.insert_op(OpKeys {
            op,
            digest,
            request,
            response,
        });
        Ok(())
    }

    /// Appends `local` (numbered from 0) unless an identical run of structs,
    /// at some offset, is already there. Returns the run's offset.
    fn intern_run(&mut self, local: Vec<KeyStruct>) -> u32 {
        let len = local.len();
        if len == 0 {
            return 0;
        }
        let fits = |at: usize| {
            local
                .iter()
                .zip(&self.types[at..at + len])
                .all(|(mine, theirs)| same_shifted(mine, theirs, at as u32))
        };
        if self.types.len() >= len
            && let Some(at) = (0..=self.types.len() - len).find(|&at| fits(at))
        {
            return at as u32;
        }
        let base = self.types.len() as u32;
        self.types
            .extend(local.into_iter().map(|s| shift_struct(s, base)));
        base
    }

    /// The index of a struct equal to `wanted`, appended when there is none.
    pub(super) fn intern_one(&mut self, wanted: KeyStruct) -> u32 {
        match self.types.iter().position(|s| *s == wanted) {
            Some(at) => at as u32,
            None => {
                self.types.push(wanted);
                (self.types.len() - 1) as u32
            }
        }
    }
}

fn shift_struct(s: KeyStruct, by: u32) -> KeyStruct {
    KeyStruct {
        fields: s
            .fields
            .into_iter()
            .map(|(name, node)| (name, node.shifted(by)))
            .collect(),
    }
}

/// Whether `stored` is `local` with every index moved up by `by`.
fn same_shifted(local: &KeyStruct, stored: &KeyStruct, by: u32) -> bool {
    local.fields.len() == stored.fields.len()
        && local
            .fields
            .iter()
            .zip(&stored.fields)
            .all(|((a, na), (b, nb))| a == b && node_shifted_eq(na, nb, by))
}

fn node_shifted_eq(local: &KeyNode, stored: &KeyNode, by: u32) -> bool {
    match (local, stored) {
        (KeyNode::Opaque, KeyNode::Opaque) => true,
        (KeyNode::Struct(a), KeyNode::Struct(b)) => a + by == *b,
        (KeyNode::List(a), KeyNode::List(b)) | (KeyNode::Page(a), KeyNode::Page(b)) => {
            node_shifted_eq(a, b, by)
        }
        _ => false,
    }
}
