//! A file that carries a `part`, `part of` or `import` line is one file of a
//! split schema, whatever the server last managed to load of it: renaming in
//! it alone could silently break the others.

use std::str::FromStr;

use tower_lsp_server::ls_types::{Position, Uri};

use crate::analyze::analyze_document;
use crate::rename::{prepare_rename, rename_ranges};
use crate::rename_error::RenameError;
use crate::state::{DocumentState, next_document_state};

fn uri() -> Uri {
    Uri::from_str("file:///shared.cstack").expect("uri should parse")
}

/// Analysed alone, as if no other file of its graph had been loaded.
fn alone(text: &str) -> DocumentState {
    let (schema, _) = analyze_document(&uri(), text);
    let state = next_document_state(None, text.to_owned(), schema);
    assert!(state.resolved().is_some() && !state.is_split());
    state
}

const AT_TYPE: Position = Position {
    line: 2,
    character: 6,
};

#[test]
fn a_file_with_a_directive_cannot_be_renamed_in_place() {
    for directive in [
        "import \"base.cstack\"",
        "part of \"schema.cstack\"",
        "part \"p.cstack\"",
    ] {
        let text = format!("// shared\n{directive}\nenum Money {{\n  X\n}}\n");
        // `analyze_document` refuses directives; the schema it last parsed is
        // what a server that loaded the graph from elsewhere would hold.
        let plain = alone("// shared\n\nenum Money {\n  X\n}\n");
        let state = DocumentState {
            text,
            analyzed: plain.analyzed,
        };
        assert!(
            matches!(
                rename_ranges(&state, AT_TYPE, "Cash"),
                Err(RenameError::MultiFile)
            ),
            "{directive}"
        );
        assert!(prepare_rename(&state, AT_TYPE).is_none(), "{directive}");
    }
}

#[test]
fn a_comment_that_mentions_a_directive_does_not_block_rename() {
    let text = "// was: import \"old.cstack\"\nmodel M {\n  id Int @id\n}\n";
    let state = alone(text);
    let at_name = Position {
        line: 1,
        character: 7,
    };
    assert!(rename_ranges(&state, at_name, "N").is_ok());
}
