//! `include_server_schema!(.., contracts = "schema.contracts.lock")`: read
//! the contract lock next to the schema and turn it into the server's
//! accepted table (cratestack#1123, EXT-14).
//!
//! Everything that can be wrong with a lock is a compile error at the
//! `contracts` literal: a missing file, a file that is not a lock, a stored
//! contract that does not hash to its key (a hand edit), and a locked
//! contract of an op that the current contract broke (the message names the
//! op and the reason, and says how to drop that op's older clients on
//! purpose). The lock file is also `include_bytes!`'d, so editing it
//! rebuilds the crate.

use std::path::PathBuf;

use proc_macro2::TokenStream;
use quote::quote;
use syn::LitStr;

/// The accepted table for `schema` under the lock at `lock_path`, and the
/// `include_bytes!` that makes cargo track the file.
pub(super) fn accepted_from_lock(
    lock_path: &LitStr,
    schema: &cratestack_core::Schema,
) -> Result<(Vec<(String, Vec<[u8; 32]>)>, TokenStream), syn::Error> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let resolved = PathBuf::from(manifest_dir).join(lock_path.value());
    let fail = |message: String| syn::Error::new(lock_path.span(), message);
    let text = std::fs::read_to_string(&resolved).map_err(|error| {
        fail(format!(
            "failed to read the contract lock {}: {error}; create it with \
             `cratestack contract lock <schema> --lock {}`",
            resolved.display(),
            lock_path.value()
        ))
    })?;
    let lock = cratestack_core::ContractLock::parse(&text)
        .map_err(|error| fail(format!("{}: {error}", resolved.display())))?;
    let table = lock
        .accepted(schema)
        .map_err(|error| fail(format!("{}:\n{error}", resolved.display())))?;
    let path = resolved.display().to_string();
    let track = quote! {
        // Makes cargo rebuild when the lock changes.
        const _: &[u8] = include_bytes!(#path);
    };
    Ok((table, track))
}

#[cfg(test)]
mod tests {
    use cratestack_core::{ContractLock, Schema};
    use proc_macro2::Span;

    use super::accepted_from_lock;

    const V1: &str = "transport rpc\n\nmodel Widget {\n  id Int @id\n  name String\n}\n";

    fn schema(source: &str) -> Schema {
        cratestack_parser::parse_schema(source).expect("parses")
    }

    /// A lock file under the temp dir, named for its test; the macro joins
    /// an absolute path onto `CARGO_MANIFEST_DIR` as itself.
    fn write(name: &str, text: &str) -> syn::LitStr {
        let path = std::env::temp_dir().join(format!("cratestack-lock-arg-{name}.lock"));
        std::fs::write(&path, text).expect("write lock");
        syn::LitStr::new(path.to_str().expect("utf-8 path"), Span::call_site())
    }

    fn locked(source: &str) -> ContractLock {
        let mut lock = ContractLock::new();
        lock.lock_generation(&schema(source), "2026-10-01", "store 1.0");
        lock
    }

    #[test]
    fn a_compatible_lock_adds_the_old_digests_after_the_current_one() {
        let lit = write("compatible", &locked(V1).to_json());
        let new = schema(&V1.replace("  name String\n", "  name String\n  note String?\n"));
        let (table, track) = accepted_from_lock(&lit, &new).expect("compatible");
        let create = &table
            .iter()
            .find(|(k, _)| k == "model.Widget.create")
            .unwrap()
            .1;
        assert_eq!(create.len(), 2);
        assert_eq!(
            create[0],
            cratestack_core::op_contract_digest(&new, "model.Widget.create").unwrap()
        );
        assert!(track.to_string().contains("include_bytes"));
    }

    #[test]
    fn an_incompatible_entry_is_an_error_naming_the_op_and_the_reason() {
        let lit = write("incompatible", &locked(V1).to_json());
        let new = schema(&V1.replace("  name String\n", ""));
        let error = accepted_from_lock(&lit, &new).unwrap_err().to_string();
        assert!(error.contains("model.Widget.create"), "{error}");
        assert!(error.contains("`Widget.name` was removed"), "{error}");
        assert!(error.contains("contract prune --op"), "{error}");
    }

    #[test]
    fn a_hand_edited_lock_is_an_error() {
        let text = locked(V1)
            .to_json()
            .replace("\"verb\": \"create\"", "\"verb\": \"forged\"");
        let error = accepted_from_lock(&write("edited", &text), &schema(V1))
            .unwrap_err()
            .to_string();
        assert!(error.contains("edited by hand"), "{error}");
    }

    #[test]
    fn a_missing_lock_is_an_error_saying_how_to_create_it() {
        let lit = syn::LitStr::new("/nonexistent/vaam.contracts.lock", Span::call_site());
        let error = accepted_from_lock(&lit, &schema(V1))
            .unwrap_err()
            .to_string();
        assert!(error.contains("contract lock"), "{error}");
        assert!(error.contains("cratestack contract lock"), "{error}");
    }
}
