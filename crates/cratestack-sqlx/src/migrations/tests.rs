use super::*;

fn migration(id: &str, up: &str) -> Migration {
    Migration {
        id: id.to_owned(),
        description: format!("migration {id}"),
        up_pre: None,
        up: up.to_owned(),
        down: None,
    }
}

#[test]
fn checksum_changes_when_up_sql_changes() {
    let a = migration("20260101000000_init", "CREATE TABLE a (id INT);");
    let mut b = a.clone();
    b.up = "CREATE TABLE a (id BIGINT);".to_owned();
    assert_ne!(a.checksum(), b.checksum());
}

#[test]
fn checksum_is_stable_for_same_inputs() {
    let a = migration("20260101000000_init", "CREATE TABLE a (id INT);");
    let b = a.clone();
    assert_eq!(a.checksum(), b.checksum());
}

/// The whole point of putting `up_pre` in the hash: an operator who
/// edits their hand-written backfill after the migration has been
/// applied must get the same drift error they would get for editing
/// `up.sql`. Before this field existed, a pre-script was invisible
/// to drift detection.
#[test]
fn checksum_changes_when_up_pre_sql_changes() {
    let mut a = migration(
        "20260101000000_init",
        "ALTER TABLE a ALTER COLUMN c SET NOT NULL;",
    );
    a.up_pre = Some("UPDATE a SET c = 0 WHERE c IS NULL;".to_owned());
    let mut b = a.clone();
    b.up_pre = Some("UPDATE a SET c = 1 WHERE c IS NULL;".to_owned());
    assert_ne!(a.checksum(), b.checksum());
}

#[test]
fn checksum_changes_when_up_pre_sql_is_added() {
    let a = migration(
        "20260101000000_init",
        "ALTER TABLE a ALTER COLUMN c SET NOT NULL;",
    );
    let mut b = a.clone();
    b.up_pre = Some("UPDATE a SET c = 0 WHERE c IS NULL;".to_owned());
    assert_ne!(a.checksum(), b.checksum());
}

/// Pins the compatibility promise in `checksum`: for a migration
/// with no `up.pre.sql` — i.e. every migration that exists in any
/// deployment predating this field — the digest is exactly
/// `sha256(id \0 description \0 up)`, byte for byte. If this test
/// fails, upgrading cratestack turns every applied migration into a
/// `ChecksumMismatch` and every deployment refuses to boot.
#[test]
fn checksum_without_up_pre_matches_the_pre_up_pre_digest() {
    let migration = migration("20260101000000_init", "CREATE TABLE a (id INT);");

    let mut legacy = Sha256::new();
    legacy.update(migration.id.as_bytes());
    legacy.update(b"\0");
    legacy.update(migration.description.as_bytes());
    legacy.update(b"\0");
    legacy.update(migration.up.as_bytes());
    let legacy: [u8; 32] = legacy.finalize().into();

    assert_eq!(migration.checksum(), legacy);
}
