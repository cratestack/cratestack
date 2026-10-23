//! Which functions a `@default(...)` may call (cratestack#1209).
//!
//! A function-shaped default reaches a migration's `DEFAULT` clause, so it
//! has to name something the database can run. `cratestack check` accepts
//! three groups and refuses everything else:
//!
//! - the markers, `dbgenerated()` and the refused `autoincrement()`; see
//!   [`super::default_markers`];
//! - cratestack's own generators, [`BuiltinDefault`]: `now()`, `uuid()` and
//!   `cuid()`. The runtime fills them in Rust on create, on every backend,
//!   and the migration emitter writes the matching database default where
//!   the database has one;
//! - functions the schema registers as provided by its database, with
//!   `datasource db { functions = ["my_fn"] }`. Registering is an assertion,
//!   like `dbgenerated()`: cratestack writes the call verbatim, and the
//!   database refuses the migration if the function does not exist.
//!
//! `@default(auth().field)` is none of these: it is filled from the auth
//! context and never reaches DDL.

use super::Datasource;
use super::default_markers::{MarkerCall, marker_call};

/// A `@default(...)` function cratestack itself defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinDefault {
    /// `now()`: the current UTC time, for a `DateTime` field.
    Now,
    /// `uuid()`: a random (v4) UUID, for a `Uuid` or `String` field.
    Uuid,
    /// `cuid()`: a cuid2-shaped id ([`crate::cuid::new_cuid`]), for a
    /// `Cuid` or `String` field.
    Cuid,
}

impl BuiltinDefault {
    pub const ALL: [Self; 3] = [Self::Now, Self::Uuid, Self::Cuid];

    /// The function's exact spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Now => "now",
            Self::Uuid => "uuid",
            Self::Cuid => "cuid",
        }
    }

    /// The builtin whose name matches, ignoring case. Postgres folds
    /// unquoted identifiers, so `UUID()` would otherwise reach the
    /// migration as a call to a function that does not exist.
    pub fn from_name_ignore_case(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|builtin| name.eq_ignore_ascii_case(builtin.name()))
    }

    /// The `.cstack` scalar types the generated value fits.
    pub fn field_types(self) -> &'static [&'static str] {
        match self {
            Self::Now => &["DateTime"],
            Self::Uuid => &["Uuid", "String"],
            Self::Cuid => &["Cuid", "String"],
        }
    }
}

/// A call at the start of a default's inner text: `name(args)trailing`.
/// `closed` is false when the opening `(` has no matching `)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FunctionCall<'a> {
    pub name: &'a str,
    pub args: &'a str,
    pub trailing: &'a str,
    pub closed: bool,
}

impl FunctionCall<'_> {
    /// `name()` and nothing else, whitespace aside.
    pub fn is_bare(&self) -> bool {
        self.closed && self.args.trim().is_empty() && self.trailing.trim().is_empty()
    }
}

/// Parse the call at the start of `inner`, or `None` if it does not start
/// with `name(`. A name is `[A-Za-z_][A-Za-z0-9_.]*`, so a schema-qualified
/// `public.my_fn()` parses too; whitespace may sit between it and `(`. The
/// closing `)` is the one that balances the opening `(`.
pub fn function_call(inner: &str) -> Option<FunctionCall<'_>> {
    let trimmed = inner.trim_start();
    if !trimmed.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    let name_len = trimmed
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .unwrap_or(trimmed.len());
    let (name, rest) = trimmed.split_at(name_len);
    let body = rest.trim_start().strip_prefix('(')?;
    let mut depth = 1usize;
    for (index, c) in body.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(FunctionCall {
                        name,
                        args: &body[..index],
                        trailing: &body[index + 1..],
                        closed: true,
                    });
                }
            }
            _ => {}
        }
    }
    Some(FunctionCall {
        name,
        args: body,
        trailing: "",
        closed: false,
    })
}

/// The functions `datasource { functions = [...] }` registers, in order.
/// `Ok(empty)` when there is no datasource or no `functions` entry; `Err`
/// with a message when the entry is not a list of quoted function names.
pub fn registered_functions(datasource: Option<&Datasource>) -> Result<Vec<&str>, String> {
    let Some(entry) =
        datasource.and_then(|datasource| datasource.entries.iter().find(|e| e.key == "functions"))
    else {
        return Ok(Vec::new());
    };
    let expected = "`functions` must be a list of quoted function names, like \
                    `functions = [\"my_fn\", \"public.other_fn\"]`";
    let list = entry
        .value
        .trim()
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| expected.to_owned())?;
    let mut names = Vec::new();
    for item in list
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let name = item
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .filter(|name| is_function_name(name))
            .ok_or_else(|| format!("{expected}; `{item}` is not one"))?;
        names.push(name);
    }
    Ok(names)
}

fn is_function_name(name: &str) -> bool {
    !name.ends_with('.')
        && !name.contains("..")
        && function_call(&format!("{name}()")).is_some_and(|c| c.name == name && c.is_bare())
}

/// What a `@default(...)` inner text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultValue<'a> {
    /// `dbgenerated()` or `autoincrement()`, in any spelling.
    Marker(MarkerCall<'a>),
    /// `auth().field`: filled from the auth context, never written to DDL.
    Auth,
    /// `now()`, `uuid()` or `cuid()`, matched ignoring case. Only the exact
    /// bare call is valid; `call` carries the spelling for that check.
    Builtin(BuiltinDefault, FunctionCall<'a>),
    /// Any other call: valid only when the function is registered.
    Function(FunctionCall<'a>),
    /// Anything that is not a call: a literal.
    Literal,
}

/// Classify a `@default(...)` inner text. The parser and the migration
/// emitter both go through this, so they cannot disagree on what a default
/// means.
pub fn classify_default(inner: &str) -> DefaultValue<'_> {
    if let Some(call) = marker_call(inner) {
        return DefaultValue::Marker(call);
    }
    let Some(call) = function_call(inner) else {
        return DefaultValue::Literal;
    };
    if call.name == "auth" {
        return DefaultValue::Auth;
    }
    match BuiltinDefault::from_name_ignore_case(call.name) {
        Some(builtin) => DefaultValue::Builtin(builtin, call),
        None => DefaultValue::Function(call),
    }
}

#[cfg(test)]
#[path = "default_functions_tests.rs"]
mod tests;
