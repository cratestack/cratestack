use std::path::Path;

use super::*;

#[test]
fn fold_removes_dots_without_climbing_past_the_root() {
    assert_eq!(
        fold(Path::new("/a/./b/../c/x.cstack")),
        Path::new("/a/c/x.cstack")
    );
    assert_eq!(fold(Path::new("/../a")), Path::new("/a"));
    assert_eq!(fold(Path::new("../a")), Path::new("../a"));
}

#[test]
fn relative_paths_climb_with_dotdot() {
    let root = Path::new("/w/app/schema.cstack");
    assert_eq!(
        root_relative(root, Path::new("/w/app/models/user.cstack")),
        "models/user.cstack"
    );
    assert_eq!(
        root_relative(root, Path::new("/w/shared/money.cstack")),
        "../shared/money.cstack"
    );
    assert_eq!(root_relative(root, root), "schema.cstack");
    assert_eq!(
        lexical_relative(Path::new("/w/app/models"), root),
        "../schema.cstack"
    );
}

#[test]
fn display_joins_the_roots_display_directory() {
    let root = Path::new("/w/app/schema.cstack");
    let part = Path::new("/w/app/models/user.cstack");
    assert_eq!(
        display_for("examples/app/schema.cstack", root, part),
        "examples/app/models/user.cstack"
    );
    assert_eq!(
        display_for("schema.cstack", root, part),
        "models/user.cstack"
    );
    assert_eq!(
        display_for("x/schema.cstack", root, root),
        "x/schema.cstack"
    );
}
