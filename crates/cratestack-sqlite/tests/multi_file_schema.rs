//! cratestack#919: `include_embedded_schema!` over a schema split across
//! four files (a `part`, an `import`, and a part under the import).
//! `SCHEMA_FILES` lists every file, and each is a rebuild dependency;
//! `.ci/multi-file-rebuild-check.sh` proves the latter with real builds.

use cratestack::RusqliteRuntime;
use cratestack::include_embedded_schema;
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};

include_embedded_schema!("tests/fixtures/multi_file/schema.cstack");

use cratestack_schema::NOTE_MODEL;
use cratestack_schema::NoteStatus;
use cratestack_schema::models::Note;

#[test]
fn models_types_enums_come_from_every_file() {
    assert_eq!(cratestack_schema::MODELS, ["Note"]);
    assert!(cratestack_schema::TYPES.contains(&"NoteSummary"));
    assert_eq!(cratestack_schema::ENUMS, ["NoteStatus"]);
}

#[test]
fn schema_files_lists_every_file_in_load_order() {
    let files = cratestack_schema::SCHEMA_FILES;
    let paths: Vec<&str> = files.iter().map(|(path, _)| *path).collect();
    assert_eq!(
        paths,
        [
            "tests/fixtures/multi_file/schema.cstack",
            "tests/fixtures/multi_file/parts/notes.cstack",
            "tests/fixtures/multi_file/shared/common.cstack",
            "tests/fixtures/multi_file/shared/common/status.cstack",
        ]
    );
    assert_eq!(
        files[0],
        (
            cratestack_schema::SCHEMA_PATH,
            cratestack_schema::SCHEMA_SOURCE
        )
    );
    for (path, contents) in files {
        let on_disk = std::fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR")))
            .expect("listed file exists");
        assert_eq!(*contents, on_disk, "{path}");
    }
}

#[test]
fn a_note_round_trips_through_sqlite() {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    let ddl = create_table_sql(&NOTE_MODEL);
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&ddl).expect("apply DDL");
            Ok(())
        })
        .unwrap();
    let delegate = ModelDelegate::<Note, i64>::new(&runtime, &NOTE_MODEL);

    let created = delegate
        .create(cratestack_schema::CreateNoteInput {
            id: 1,
            title: "first".to_owned(),
            status: NoteStatus::Published,
        })
        .run()
        .expect("create succeeds");
    assert_eq!(created.status, NoteStatus::Published);
    // The mixin field, defined in the imported file, is on the model.
    let _stamp: &_ = &created.createdAt;

    let found = delegate
        .find_unique(1)
        .run()
        .expect("find succeeds")
        .expect("row exists");
    assert_eq!(found.title, "first");
    assert_eq!(found.status, NoteStatus::Published);
}

/// The rebuild script adds a part declaring `RebuildProbe` and sets the
/// env var; without it the type must be absent.
#[test]
fn rebuild_probe_type_is_present_only_when_requested() {
    let present = cratestack_schema::TYPES.contains(&"RebuildProbe");
    assert_eq!(
        present,
        std::env::var_os("CRATESTACK_REBUILD_PROBE").is_some()
    );
}
