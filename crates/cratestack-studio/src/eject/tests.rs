use std::fs;
use std::path::Path;

use super::*;

fn assert_file<S: AsRef<str>>(path: &Path, expected_substring: S) {
    let bytes = fs::read(path).expect("read");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains(expected_substring.as_ref()),
        "{path:?} did not contain {:?}; full body:\n{text}",
        expected_substring.as_ref(),
    );
}

#[test]
fn ejects_starter_project_into_empty_directory() {
    let temp = tempfile::tempdir().expect("temp");
    let out = temp.path().join("my-app");
    let report = eject(&EjectOptions {
        out: out.clone(),
        name: None,
        force: false,
        with_ui: false,
    })
    .expect("eject succeeds into empty dir");

    assert!(out.join("Cargo.toml").is_file());
    assert!(out.join("README.md").is_file());
    assert!(out.join("studio.toml").is_file());
    assert!(out.join("schemas/example.cstack").is_file());
    assert!(out.join("src/main.rs").is_file());
    assert!(
        !out.join("ui").exists(),
        "ui/ should only land with --with-ui"
    );

    assert_file(&out.join("Cargo.toml"), "name = \"my-app\"");
    assert_file(&out.join("Cargo.toml"), "cratestack-studio = \"");
    assert_file(&out.join("README.md"), "# my-app");
    assert_file(&out.join("src/main.rs"), "cratestack_studio::");
    assert!(!report.with_ui);
}

#[test]
fn explicit_name_overrides_directory_basename() {
    let temp = tempfile::tempdir().expect("temp");
    let out = temp.path().join("anything");
    eject(&EjectOptions {
        out: out.clone(),
        name: Some("renamed-app".into()),
        force: false,
        with_ui: false,
    })
    .expect("eject succeeds");
    assert_file(&out.join("Cargo.toml"), "name = \"renamed-app\"");
    assert_file(&out.join("README.md"), "# renamed-app");
}

#[test]
fn refuses_non_empty_directory_without_force() {
    let temp = tempfile::tempdir().expect("temp");
    std::fs::write(temp.path().join("existing.txt"), "hi").unwrap();
    let error = eject(&EjectOptions {
        out: temp.path().to_path_buf(),
        name: None,
        force: false,
        with_ui: false,
    })
    .expect_err("non-empty dir should refuse");
    assert!(matches!(error, EjectError::OutputNotEmpty { .. }));
}

#[test]
fn force_overwrites_existing_files() {
    let temp = tempfile::tempdir().expect("temp");
    let target = temp.path().join("Cargo.toml");
    std::fs::write(&target, "PLACEHOLDER").unwrap();
    eject(&EjectOptions {
        out: temp.path().to_path_buf(),
        name: Some("forced".into()),
        force: true,
        with_ui: false,
    })
    .expect("force eject succeeds");
    assert_file(&target, "name = \"forced\"");
}

#[test]
fn with_ui_unpacks_leptos_sources() {
    let temp = tempfile::tempdir().expect("temp");
    let out = temp.path().join("with-ui");
    let report = eject(&EjectOptions {
        out: out.clone(),
        name: None,
        force: false,
        with_ui: true,
    })
    .expect("eject with ui succeeds");
    assert!(report.with_ui);
    assert!(out.join("ui/Cargo.toml").is_file());
    assert!(out.join("ui/Trunk.toml").is_file());
    assert!(out.join("ui/index.html").is_file());
    assert!(out.join("ui/src/lib.rs").is_file());
}
