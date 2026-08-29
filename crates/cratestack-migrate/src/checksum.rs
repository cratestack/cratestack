//! SHA-256 checksum over a [`Projections`] value — used by
//! `cratestack migrate baseline` (issue #205) as the fingerprint
//! embedded in the synthetic `cratestack_migrations` row it seeds, so
//! two baseline runs over the same table set are only equal if the
//! introspected shape they saw was byte-identical. Deliberately pure
//! and DB-free (no `postgres-introspect` feature needed) so it's
//! testable without Postgres and usable from the snapshot-only path
//! too.

use sha2::{Digest, Sha256};

use crate::MigrateError;
use crate::projection::Projections;

/// Hex-encoded SHA-256 of `projections`'s canonical JSON encoding.
///
/// Deterministic across runs of the same `cratestack-migrate` build —
/// every map inside [`Projections`] is a `BTreeMap`, so key order
/// never varies. Not guaranteed stable across crate versions if the
/// IR shape itself changes; this is a within-adoption drift
/// fingerprint, not a content-addressed identity meant to survive
/// upgrades.
pub fn projections_checksum(projections: &Projections) -> Result<String, MigrateError> {
    let mut hasher = Sha256::new();
    serde_json::to_writer(ChecksumWriter(&mut hasher), projections)
        .map_err(MigrateError::ChecksumSerialize)?;
    // sha2 0.11 / digest 0.11 return `hybrid_array::Array`, which (unlike
    // digest 0.10's `GenericArray`) implements no `LowerHex`. The
    // byte-wise `{:02x}` fold below is this repo's existing hex idiom
    // (`cratestack-core/src/transport.rs`) and is byte-for-byte what
    // `format!("{:x}", …)` produced — this string is persisted/keyed on,
    // so it must not change shape.
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Feeds serialized JSON bytes straight into the hasher instead of
/// materializing an intermediate `Vec<u8>` — `Projections` for a
/// large baselined database can be sizeable.
struct ChecksumWriter<'a>(&'a mut Sha256);

impl std::io::Write for ChecksumWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::projections_checksum;
    use crate::TableProjection;
    use crate::projection::Projections;

    fn table(name: &str) -> TableProjection {
        TableProjection {
            name: name.to_owned(),
            rename_from: None,
            columns: Vec::new(),
            column_renames: Vec::new(),
            indexes: Vec::new(),
            checks: Vec::new(),
            foreign_keys: Vec::new(),
        }
    }

    #[test]
    fn checksum_is_stable_for_identical_projections() {
        let mut a = Projections::default();
        a.tables.insert("users".to_owned(), table("users"));
        let mut b = Projections::default();
        b.tables.insert("users".to_owned(), table("users"));

        assert_eq!(
            projections_checksum(&a).unwrap(),
            projections_checksum(&b).unwrap()
        );
    }

    #[test]
    fn checksum_changes_when_shape_changes() {
        let mut a = Projections::default();
        a.tables.insert("users".to_owned(), table("users"));
        let mut b = Projections::default();
        b.tables.insert("users".to_owned(), table("users"));
        b.tables.insert("orders".to_owned(), table("orders"));

        assert_ne!(
            projections_checksum(&a).unwrap(),
            projections_checksum(&b).unwrap()
        );
    }

    #[test]
    fn checksum_is_hex_encoded_sha256() {
        let checksum = projections_checksum(&Projections::default()).unwrap();
        assert_eq!(checksum.len(), 64, "SHA-256 hex should be 64 chars");
        assert!(checksum.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
