use include_dir::{Dir, include_dir};

use super::{Migration, is_effectively_blank, migrations_from_dir};

static FIXTURE_MIGRATIONS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/tests/fixtures/migrations");

fn migration<'a>(migrations: &'a [Migration], id: &str) -> &'a Migration {
    migrations
        .iter()
        .find(|m| m.id == id)
        .unwrap_or_else(|| panic!("fixture has a `{id}` migration"))
}

#[test]
fn loads_migrations_in_timestamp_order() {
    let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
    let ids: Vec<_> = migrations.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "20260101000000_init",
            "20260102000000_add_index",
            "20260103000000_backfill_owner",
            "20260104000000_scaffold_untouched",
        ]
    );
}

/// cratestack#843: before this, an operator could write `up.pre.sql`
/// exactly as the generated warning instructed and the file would be
/// silently ignored — the migration then failed at deploy time with
/// the very NOT NULL violation the backfill was meant to prevent.
#[test]
fn reads_up_pre_sql_when_it_has_real_sql() {
    let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
    let backfill = migration(&migrations, "20260103000000_backfill_owner");

    assert_eq!(
        backfill.up_pre.as_deref(),
        Some(
            "-- Backfill so the NOT NULL promotion in up.sql can succeed.\n\
             UPDATE widgets SET owner = 'unknown' WHERE owner IS NULL;\n"
        )
    );
}

/// A scaffold the operator never filled in must be indistinguishable
/// from no file at all — otherwise scaffolding `up.pre.sql` for an
/// existing blocking migration would change its checksum and make
/// every deployment report drift.
#[test]
fn comment_only_up_pre_sql_reads_as_absent() {
    let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
    assert_eq!(
        migration(&migrations, "20260104000000_scaffold_untouched").up_pre,
        None
    );
}

#[test]
fn untouched_scaffold_does_not_change_the_checksum() {
    let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
    let scaffolded = migration(&migrations, "20260104000000_scaffold_untouched");

    let mut without_the_file = scaffolded.clone();
    without_the_file.up_pre = None;

    assert_eq!(scaffolded.checksum(), without_the_file.checksum());
}

#[test]
fn absent_up_pre_sql_is_none() {
    let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
    assert_eq!(migration(&migrations, "20260101000000_init").up_pre, None);
}

#[test]
fn blankness_is_judged_on_executable_sql_not_emptiness() {
    assert!(is_effectively_blank(""));
    assert!(is_effectively_blank("\n  \n"));
    assert!(is_effectively_blank(
        "-- just a comment\n--   and another\n"
    ));
    assert!(!is_effectively_blank("-- a comment\nUPDATE t SET c = 0;\n"));
    assert!(!is_effectively_blank("UPDATE t SET c = 0; -- trailing\n"));
}

#[test]
fn reads_down_sql_when_present_and_none_when_absent() {
    let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);

    let init = migrations
        .iter()
        .find(|m| m.id == "20260101000000_init")
        .expect("fixture has an init migration");
    assert!(init.up.contains("CREATE TABLE widgets"));
    assert_eq!(init.down.as_deref(), Some("DROP TABLE widgets;\n"));

    let add_index = migrations
        .iter()
        .find(|m| m.id == "20260102000000_add_index")
        .expect("fixture has an add_index migration");
    assert!(add_index.up.contains("CREATE INDEX"));
    assert_eq!(add_index.down, None);
}

#[test]
fn description_defaults_to_the_id() {
    let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
    for migration in &migrations {
        assert_eq!(migration.description, migration.id);
    }
}
