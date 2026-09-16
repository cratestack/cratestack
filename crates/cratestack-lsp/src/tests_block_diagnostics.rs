use std::str::FromStr;

use tower_lsp_server::ls_types::{Position, Range, Uri};

use crate::analyze::analyze_document;

#[test]
fn block_diagnostics_select_the_name_with_lf_and_crlf() {
    let Ok(uri) = Uri::from_str("file:///schema.cstack") else {
        unreachable!("the fixture URI is a literal")
    };
    for newline in ["\n", "\r\n"] {
        for (kind, body) in [
            ("datasource", "  provider = \"postgresql\""),
            ("auth", "  id String"),
        ] {
            for name in ["source", "part", "import"] {
                let text =
                    format!("// café\n\n  {kind} {name} {{\n{body}\n}}").replace('\n', newline);
                let (schema, diagnostics) = analyze_document(&uri, &text);
                if name == "source" {
                    assert!(schema.is_some());
                    assert!(diagnostics.is_empty(), "{diagnostics:?}");
                } else {
                    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
                    let start = (kind.len() + 3) as u32;
                    assert_eq!(
                        diagnostics[0].range,
                        Range {
                            start: Position::new(2, start),
                            end: Position::new(2, start + name.len() as u32),
                        }
                    );
                    assert!(
                        diagnostics[0]
                            .message
                            .contains("reserved for multi-file schemas")
                    );
                }
            }
        }
    }
}
