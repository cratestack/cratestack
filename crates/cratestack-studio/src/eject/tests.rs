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
fn ejects_into_empty_directory() {
    let temp = tempfile::tempdir().expect("temp");
    let report = eject(&EjectOptions {
        out: temp.path().to_path_buf(),
        force: false,
    })
    .expect("eject succeeds into empty dir");

    assert!(temp.path().join("Cargo.toml").is_file());
    assert!(temp.path().join("index.html").is_file());
    assert!(temp.path().join("Trunk.toml").is_file());
    assert!(temp.path().join("src/lib.rs").is_file());
    assert!(temp.path().join("src/app.rs").is_file());
    assert!(temp.path().join("src/api.rs").is_file());
    assert!(temp.path().join("src/types.rs").is_file());

    assert_file(&temp.path().join("README.md"), "cratestack studio eject");

    let has_lib = report.written.iter().any(|p| p.ends_with("src/lib.rs"));
    assert!(has_lib, "report should list src/lib.rs; got {:?}", report.written);
}

#[test]
fn refuses_non_empty_directory_without_force() {
    let temp = tempfile::tempdir().expect("temp");
    std::fs::write(temp.path().join("existing.txt"), "hi").unwrap();
    let error = eject(&EjectOptions {
        out: temp.path().to_path_buf(),
        force: false,
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
        force: true,
    })
    .expect("force eject succeeds");
    let body = std::fs::read_to_string(&target).unwrap();
    assert!(body.contains("cratestack-studio-ui"), "{body}");
}

#[test]
fn skips_dist_and_target() {
    let temp = tempfile::tempdir().expect("temp");
    eject(&EjectOptions {
        out: temp.path().to_path_buf(),
        force: false,
    })
    .expect("eject succeeds");
    assert!(!temp.path().join("dist").exists(), "dist/ must not be ejected");
    assert!(
        !temp.path().join("target").exists(),
        "target/ must not be ejected"
    );
}
