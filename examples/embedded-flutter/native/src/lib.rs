#![allow(non_snake_case)]

//! Rust side of the `embedded-flutter` example.
//!
//! The Flutter UI hands user actions to a small typed API (`api::notes`)
//! which is bridged into Dart by [`flutter_rust_bridge`][frb]. Under the
//! hood it's the same `include_embedded_schema!` + `ModelDelegate`
//! shape every other embedded example uses — the only difference is
//! the JS-shaped facade types we return to Dart.
//!
//! [frb]: https://cjycode.com/flutter_rust_bridge/

// `mod frb_generated;` is added by `flutter_rust_bridge_codegen generate`.
// It pulls in the auto-generated Rust glue under `frb_generated.rs` that
// wraps each `pub fn` in `api/` with a C-ABI export the Dart side calls.
mod frb_generated;

pub mod api;
pub mod schema;

#[cfg(test)]
mod tests {
    use super::api::notes;
    use super::schema;
    use chrono::Utc;
    use cratestack_rusqlite::ddl::create_table_sql;
    use cratestack_rusqlite::{ModelDelegate, RusqliteRuntime};
    use uuid::Uuid;

    #[test]
    fn note_crud_round_trip_in_memory() {
        let runtime = RusqliteRuntime::open_in_memory().unwrap();
        runtime
            .with_connection(|conn| {
                conn.execute_batch(&create_table_sql(&schema::cratestack_schema::NOTE_MODEL))?;
                Ok(())
            })
            .unwrap();
        let model = ModelDelegate::<schema::cratestack_schema::Note, Uuid>::new(
            &runtime,
            &schema::cratestack_schema::NOTE_MODEL,
        );
        let now = Utc::now();
        let created = model
            .create(schema::cratestack_schema::CreateNoteInput {
                id: Uuid::new_v4(),
                title: "Flutter!".into(),
                body: "first note from Dart".into(),
                pinned: false,
                completed: false,
                createdAt: now,
                updatedAt: now,
            })
            .run()
            .unwrap();
        let view = notes::note_to_view(created);
        assert_eq!(view.title, "Flutter!");
        let listed = model.find_many().run().unwrap();
        assert_eq!(listed.len(), 1);
    }
}
