use tempfile::TempDir;

use super::*;

pub(super) fn file(name: &str, contents: &str) -> GeneratedFile {
    GeneratedFile {
        file_name: name.to_owned(),
        contents: contents.to_owned(),
    }
}

#[test]
fn no_drift_when_disk_matches_generated_output() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("index.ts"), "export const x = 1;\n").unwrap();

    let drift = diff_generated_files(dir.path(), &[file("index.ts", "export const x = 1;\n")]);
    assert!(drift.is_empty());
}

#[test]
fn flags_hand_edited_file_as_modified() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("index.ts"), "export const x = 999;\n").unwrap();

    let drift = diff_generated_files(dir.path(), &[file("index.ts", "export const x = 1;\n")]);
    assert_eq!(
        drift,
        vec![DriftEntry {
            file_name: "index.ts".to_owned(),
            kind: DriftKind::Modified,
        }]
    );
}

#[test]
fn flags_new_generated_file_as_missing() {
    let dir = TempDir::new().expect("tempdir");

    let drift = diff_generated_files(dir.path(), &[file("index.ts", "export const x = 1;\n")]);
    assert_eq!(
        drift,
        vec![DriftEntry {
            file_name: "index.ts".to_owned(),
            kind: DriftKind::Missing,
        }]
    );
}

#[test]
fn flags_stale_disk_file_as_unexpected() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("index.ts"), "export const x = 1;\n").unwrap();
    std::fs::write(dir.path().join("stale.ts"), "export const y = 2;\n").unwrap();

    let drift = diff_generated_files(dir.path(), &[file("index.ts", "export const x = 1;\n")]);
    assert_eq!(
        drift,
        vec![DriftEntry {
            file_name: "stale.ts".to_owned(),
            kind: DriftKind::Unexpected,
        }]
    );
}

#[test]
fn check_drift_ok_when_clean() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("index.ts"), "export const x = 1;\n").unwrap();

    check_drift(
        dir.path(),
        &[file("index.ts", "export const x = 1;\n")],
        "TypeScript",
    )
    .expect("no drift");
}

#[test]
fn check_drift_errors_and_lists_files_when_dirty() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("index.ts"), "export const x = 999;\n").unwrap();

    let error = check_drift(
        dir.path(),
        &[file("index.ts", "export const x = 1;\n")],
        "TypeScript",
    )
    .expect_err("drift should fail check");
    assert!(error.to_string().contains("modified: index.ts"));
}

#[test]
fn check_drift_leaves_disk_untouched() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("index.ts"), "export const x = 999;\n").unwrap();

    let _ = check_drift(
        dir.path(),
        &[file("index.ts", "export const x = 1;\n")],
        "TypeScript",
    );

    assert_eq!(
        std::fs::read_to_string(dir.path().join("index.ts")).unwrap(),
        "export const x = 999;\n"
    );
}
