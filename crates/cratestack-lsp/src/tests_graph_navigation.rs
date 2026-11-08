//! Hover, highlights and friends in a part whose spans start past zero
//! (cratestack#921): the schema addresses the merged text, the buffer is one
//! file of it.

use tower_lsp_server::ls_types::{HoverContents, Position};

use crate::document_symbols::document_symbols_for;
use crate::hover_render::hover_at;
use crate::navigation::highlights;
use crate::semantic_tokens::semantic_tokens_for;
use crate::tests_graph_nav_support::{split, with_import};
use crate::tests_graph_support::{A, B};

#[test]
fn hover_in_a_part_with_a_base_describes_the_symbol_under_the_cursor() {
    let fixture = split();
    // `b B` in model C: the second `B` of b.cstack's `b B` lines is the type.
    let position = fixture.position(B, "b B", 0);
    let position = Position::new(position.line, position.character + 2);
    let hover = hover_at(fixture.document(B), position).expect("hover on B");
    let HoverContents::Markup(markup) = hover.contents else {
        panic!("markdown expected");
    };
    assert!(markup.value.contains("enum"), "{}", markup.value);
    assert!(markup.value.contains("The kind."), "{}", markup.value);
    let range = hover.range.expect("range in b.cstack");
    assert_eq!(fixture.slice(B, range), "B");
}

#[test]
fn highlights_in_a_part_are_this_files_mentions_only() {
    let fixture = split();
    let position = fixture.position(B, "enum B", 0);
    let position = Position::new(position.line, position.character + 4);
    let found = highlights(fixture.document(B), position).expect("highlights");
    // The declaration and `C.b`'s type; a.cstack's mention is another file.
    assert_eq!(found.len(), 2, "{found:?}");
    for highlight in &found {
        assert_eq!(fixture.slice(B, highlight.range), "B");
    }
}

#[test]
fn hover_on_an_imported_type_shows_its_docs() {
    let fixture = with_import();
    let position = fixture.position(A, "cur Currency", 0);
    let position = Position::new(position.line, position.character + 8);
    let hover = hover_at(fixture.document(A), position).expect("hover on Currency");
    let HoverContents::Markup(markup) = hover.contents else {
        panic!("markdown expected");
    };
    assert!(markup.value.contains("A currency."), "{}", markup.value);
    // The declaration lives in money.cstack; the hover is anchored where the
    // cursor is, or nowhere.
    if let Some(range) = hover.range {
        assert_eq!(range.start.line, 4);
    }
}

#[test]
fn document_symbols_list_only_this_files_declarations() {
    let fixture = split();
    let names = |path| -> Vec<String> {
        document_symbols_for(fixture.document(path))
            .expect("symbols")
            .into_iter()
            .map(|symbol| symbol.name)
            .collect()
    };
    assert_eq!(names(A), ["A"]);
    assert_eq!(names(B), ["C"]);
    let symbols = document_symbols_for(fixture.document(B)).unwrap();
    // `model C` is the sixth line (0-based 5) after the `part of` header.
    let line = fixture
        .text(B)
        .lines()
        .position(|l| l == "model C {")
        .unwrap();
    assert_eq!(symbols[0].range.start.line as usize, line);
    assert_eq!(symbols[0].children.as_ref().unwrap().len(), 2);
}

#[test]
fn semantic_tokens_cover_only_this_files_declarations() {
    let fixture = split();
    let tokens = semantic_tokens_for(fixture.document(A)).expect("tokens");
    // A, id, Int, @id, b, B -- and nothing from b.cstack's enum and model.
    assert_eq!(tokens.len(), 6, "{tokens:?}");
    let a_line = fixture
        .text(A)
        .lines()
        .position(|l| l == "model A {")
        .unwrap();
    assert_eq!(
        (
            tokens[0].delta_line as usize,
            tokens[0].delta_start,
            tokens[0].length
        ),
        (a_line, 6, 1)
    );
}
