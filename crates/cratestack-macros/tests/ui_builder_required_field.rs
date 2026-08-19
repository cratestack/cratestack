//! Compile-fail proof that the typestate builder's core guarantee is
//! real: omitting a required field and calling `.build()` on a generated
//! struct's builder is a *compile* error ("no method named `build`"),
//! never a runtime `Result` — see `cratestack-core/src/builder.rs`'s
//! module doc for the encoding and `cratestack-macros/src/builder.rs`
//! for the emitter this fixture is testing against.
//!
//! **Why `db = None` + a `type` block, not `Create{Model}Input`:** every
//! struct-shaped generated type shares the exact same `generate_builder`
//! call (`model_builder_fields`/`scoped_builder_fields` both derive
//! `BuilderField`s the identical way — see that module's doc), so any one
//! of them proves the mechanism. `Create{Model}Input` needs a `model`
//! block, which needs `db = Postgres` (models are rejected under
//! `db = None`, cratestack#327) — and a *fully expanding* `db = Postgres`
//! schema needs `::cratestack::sqlx::*` paths to resolve, which this
//! crate's dev-dependency (`cratestack-api`, deliberately DB-less — see
//! `Cargo.toml`) can't provide. `type Widget { .. }` gets a builder
//! through the identical code path (`types.rs`'s `generate_type_struct`)
//! and compiles cleanly under `db = None` + the existing `cratestack-api`
//! dev-dependency, exactly like `ui_procedure_registry_witness.rs`'s
//! fixture — see that file's module doc for the fuller rationale on why
//! `db = None` is the only schema shape this crate's trybuild sandbox can
//! fully expand.
//!
//! Same fixture-staging trick and checked-in `.stderr` snapshot
//! requirement as the other three `ui_*.rs` suites — see `ui.rs`'s module
//! doc for the path-length rationale and
//! `ui_procedure_registry_witness.rs`'s module doc for why the snapshot
//! isn't optional (`trybuild::Runner::run`'s `wip` panic on `Drop`).
//!
//! **The decisive check this file exists to make possible:** temporarily
//! break `cratestack-macros/src/builder.rs` so `build()` is emitted
//! unconditionally (not just on the all-`Set` state) and confirm this
//! test starts *failing* (the fixture now compiles, so `t.compile_fail`
//! reports it as a failure) — then restore it and confirm this test
//! passes again. A compile-fail test that was never watched fail proves
//! nothing; see the task's own "decisive check" note.

use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_STAGING_DIR: &str = "/tmp/cratestack-macros-ui-builder-required-field";

#[test]
fn builder_missing_required_field_does_not_compile() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let generated_dir = manifest_dir.join("tests/ui/generated");
    fs::create_dir_all(&generated_dir).expect("create tests/ui/generated");

    let staging_dir = Path::new(FIXTURE_STAGING_DIR);
    fs::create_dir_all(staging_dir).expect("create fixture staging dir");

    let staged = stage_fixture(
        &manifest_dir,
        staging_dir,
        "tests/fixtures/builder_required_field.cstack",
    );
    let staged = staged
        .to_str()
        .expect("fixture staging path should be valid UTF-8");

    let source = format!(
        r#"cratestack::include_server_schema!({staged:?}, db = None);

fn main() {{
    // `Widget` has two required fields (`id`, `name`) — `.name(..)` is
    // never called, so `WidgetBuilder`'s second state slot stays `Unset`
    // and `.build()` must not exist on that type at all.
    let _ = cratestack_schema::Widget::builder()
        .id(1)
        .build();
}}
"#
    );
    let fixture_path = generated_dir.join("builder_required_field_missing.rs");
    fs::write(&fixture_path, source).expect("write generated trybuild fixture");

    let t = trybuild::TestCases::new();
    t.compile_fail(fixture_path);
}

/// Same helper as `ui.rs`/`ui_semantic.rs`/`ui_procedure_registry_witness.rs`
/// — see `ui.rs`'s module doc for why the fixture references a staged
/// copy rather than the checked-in original directly.
fn stage_fixture(manifest_dir: &Path, staging_dir: &Path, relative_schema_path: &str) -> PathBuf {
    let source = manifest_dir.join(relative_schema_path);
    let file_name = source
        .file_name()
        .expect("fixture schema path should have a file name");
    let staged = staging_dir.join(file_name);
    fs::copy(&source, &staged).unwrap_or_else(|error| {
        panic!(
            "copy fixture {} to {}: {error}",
            source.display(),
            staged.display()
        )
    });
    staged
}
