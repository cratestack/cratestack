//! Compile-fail UI tests for semantic errors in `.cstack` schemas through
//! the three entry macros (include_server_schema!, include_embedded_schema!,
//! include_client_schema!). These ensure that bad schemas produce clean
//! `compile_error!` diagnostics, not proc-macro panics — covering issues like
//! unknown relation targets, duplicate model/field names, malformed policy
//! expressions (cratestack#420), and an out-of-range `@status(<code>)`
//! procedure attribute (cratestack#407, test 5 below).
//!
//! Malformed policy expressions are only ever interpreted by
//! `cratestack-policy`'s predicate parser (`crates/cratestack-macros/src/
//! policy/`), which is wired into model/view descriptor generation for the
//! server and embedded composers only — `include_client_schema!` never reads
//! `@@allow`/`@@deny` content at all (it's a pure HTTP-stub client with no
//! policy enforcement of its own, see `include/client/grpc/client_struct.rs`'s
//! doc comment). `include_server_schema!(db = Postgres)` is *also* not
//! usable for this in this crate's own trybuild sandbox: that composer
//! checks `guard_server_postgres_backend`'s "compiled without the
//! `postgres` feature" gate (see `ui.rs`'s extension-gating tests, which
//! rely on that exact feature being off here) before it ever reaches model
//! descriptor / policy codegen, so a malformed predicate would be masked by
//! the feature-gate diagnostic instead of the policy-parser one. So the
//! malformed-policy case below is routed through `include_embedded_schema!`
//! (test 4, alongside test 2's duplicate-model case — embedded has no such
//! feature gate in front of policy codegen); the client slot (test 3)
//! instead covers a duplicate-field-name schema, honestly named.
//!
//! Uses the same fixture staging and path-fixing logic as `ui.rs` — see that
//! file's module doc for details on why `FIXTURE_STAGING_DIR` is necessary.

use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_STAGING_DIR: &str = "/tmp/cratestack-macros-ui-semantic-420";

#[test]
fn semantic_error_compile_fail() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let generated_dir = manifest_dir.join("tests/ui/generated");
    fs::create_dir_all(&generated_dir).expect("create tests/ui/generated");

    let staging_dir = Path::new(FIXTURE_STAGING_DIR);
    fs::create_dir_all(staging_dir).expect("create fixture staging dir");

    let t = trybuild::TestCases::new();

    // Test 1: include_server_schema! with unknown relation target
    write_server_fixture(
        &manifest_dir,
        staging_dir,
        &generated_dir,
        "semantic_error_unknown_relation.rs",
        "tests/fixtures/semantic_error_unknown_relation.cstack",
    );
    t.compile_fail(generated_dir.join("semantic_error_unknown_relation.rs"));

    // Test 2: include_embedded_schema! with duplicate model
    write_embedded_fixture(
        &manifest_dir,
        staging_dir,
        &generated_dir,
        "semantic_error_duplicate_model.rs",
        "tests/fixtures/semantic_error_duplicate_model.cstack",
    );
    t.compile_fail(generated_dir.join("semantic_error_duplicate_model.rs"));

    // Test 3: include_client_schema! with duplicate field name
    write_client_fixture(
        &manifest_dir,
        staging_dir,
        &generated_dir,
        "semantic_error_duplicate_field.rs",
        "tests/fixtures/semantic_error_duplicate_field.cstack",
    );
    t.compile_fail(generated_dir.join("semantic_error_duplicate_field.rs"));

    // Test 4: include_embedded_schema! with a malformed @@allow policy
    // expression (unbalanced parens in the predicate) — neither the client
    // macro (no policy codegen at all) nor the server macro (masked by the
    // postgres-feature gate in this sandbox) can exercise this, see the
    // module doc above.
    write_embedded_fixture(
        &manifest_dir,
        staging_dir,
        &generated_dir,
        "semantic_error_malformed_policy.rs",
        "tests/fixtures/semantic_error_malformed_policy.cstack",
    );
    t.compile_fail(generated_dir.join("semantic_error_malformed_policy.rs"));

    // Test 5 (cratestack#407): `@status(<code>)` outside the allowed
    // `200..=299` range is a schema-compile-time error, not a runtime
    // surprise — see `cratestack-parser`'s
    // `validate_procedure_status_attribute`.
    write_embedded_fixture(
        &manifest_dir,
        staging_dir,
        &generated_dir,
        "semantic_error_status_out_of_range.rs",
        "tests/fixtures/semantic_error_status_out_of_range.cstack",
    );
    t.compile_fail(generated_dir.join("semantic_error_status_out_of_range.rs"));
}

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

fn write_server_fixture(
    manifest_dir: &Path,
    staging_dir: &Path,
    generated_dir: &Path,
    file_name: &str,
    relative_schema_path: &str,
) {
    let staged = stage_fixture(manifest_dir, staging_dir, relative_schema_path);
    let staged = staged
        .to_str()
        .expect("fixture staging path should be valid UTF-8");
    let source = format!(
        "cratestack_macros::include_server_schema!({staged:?}, db = Postgres);\n\nfn main() {{}}\n"
    );
    fs::write(generated_dir.join(file_name), source).expect("write generated trybuild fixture");
}

fn write_embedded_fixture(
    manifest_dir: &Path,
    staging_dir: &Path,
    generated_dir: &Path,
    file_name: &str,
    relative_schema_path: &str,
) {
    let staged = stage_fixture(manifest_dir, staging_dir, relative_schema_path);
    let staged = staged
        .to_str()
        .expect("fixture staging path should be valid UTF-8");
    let source =
        format!("cratestack_macros::include_embedded_schema!({staged:?});\n\nfn main() {{}}\n");
    fs::write(generated_dir.join(file_name), source).expect("write generated trybuild fixture");
}

fn write_client_fixture(
    manifest_dir: &Path,
    staging_dir: &Path,
    generated_dir: &Path,
    file_name: &str,
    relative_schema_path: &str,
) {
    let staged = stage_fixture(manifest_dir, staging_dir, relative_schema_path);
    let staged = staged
        .to_str()
        .expect("fixture staging path should be valid UTF-8");
    let source =
        format!("cratestack_macros::include_client_schema!({staged:?});\n\nfn main() {{}}\n");
    fs::write(generated_dir.join(file_name), source).expect("write generated trybuild fixture");
}
