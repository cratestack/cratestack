//! Go-to-definition, completion and rename in a schema split across files
//! (cratestack#921).

use crate::completion::completion_items;
use crate::definition_graph::definition_in;
use crate::rename::{prepare_rename, rename_ranges, workspace_edit};
use crate::tests_graph_nav_support::{MONEY, split, with_import};
use crate::tests_graph_support::{A, B, uri};

#[test]
fn goto_definition_jumps_to_a_declaration_in_another_part() {
    let fixture = split();
    // `b B` in a.cstack: the type `B` is the enum declared in b.cstack.
    let position = fixture.position(A, "b B", 0);
    let position = tower_lsp_server::ls_types::Position::new(position.line, position.character + 2);
    let view = fixture.document(A).view().expect("a.cstack parsed");
    let location = definition_in(&uri(A), &view, position).expect("definition");
    assert_eq!(location.uri, uri(B));
    assert_eq!(fixture.slice(B, location.range), "B");
    let enum_line = fixture
        .text(B)
        .lines()
        .position(|l| l == "enum B {")
        .unwrap();
    assert_eq!(location.range.start.line as usize, enum_line);
}

#[test]
fn goto_definition_stays_in_the_file_for_a_local_declaration() {
    let fixture = split();
    let position = fixture.position(B, "b B", 0);
    let position = tower_lsp_server::ls_types::Position::new(position.line, position.character + 2);
    let view = fixture.document(B).view().expect("b.cstack parsed");
    let location = definition_in(&uri(B), &view, position).expect("definition");
    assert_eq!(location.uri, uri(B));
    assert_eq!(fixture.slice(B, location.range), "B");
}

#[test]
fn goto_definition_jumps_into_an_import() {
    let fixture = with_import();
    let position = fixture.position(A, "cur Currency", 0);
    let position = tower_lsp_server::ls_types::Position::new(position.line, position.character + 8);
    let view = fixture.document(A).view().expect("a.cstack parsed");
    let location = definition_in(&uri(A), &view, position).expect("definition");
    assert_eq!(location.uri, uri(MONEY));
    assert_eq!(fixture.slice(MONEY, location.range), "Currency");
    assert_eq!(location.range.start.line, 1);
}

#[test]
fn completion_offers_declarations_from_every_file() {
    let fixture = split();
    let view = fixture.document(A).view().expect("a.cstack parsed");
    let labels: Vec<String> = completion_items(Some(view.schema), None)
        .into_iter()
        .map(|item| item.label)
        .collect();
    // A is this file's; C is declared in b.cstack.
    for expected in ["A", "C"] {
        assert!(
            labels.contains(&expected.to_owned()),
            "{expected} in {labels:?}"
        );
    }
    let imported = with_import();
    let view = imported.document(A).view().expect("a.cstack parsed");
    let labels: Vec<String> = completion_items(Some(view.schema), None)
        .into_iter()
        .map(|item| item.label)
        .collect();
    assert!(labels.contains(&"Stamps".to_owned()), "{labels:?}");
}

#[test]
fn rename_is_refused_in_a_split_schema() {
    let fixture = split();
    let position = fixture.position(B, "enum B", 0);
    let position = tower_lsp_server::ls_types::Position::new(position.line, position.character + 4);
    let document = fixture.document(B);

    assert!(prepare_rename(document, position).is_none());
    let error = rename_ranges(document, position, "Kind").expect_err("refused");
    let wire: tower_lsp_server::jsonrpc::Error = error.into();
    assert_eq!(
        wire.message,
        "rename is not available yet in a schema split across files"
    );
    assert!(workspace_edit(uri(B), document, position, "Kind").is_err());
}
