//! Block names must retain byte-accurate spans without re-reading source during validation.

use cratestack_core::{Schema, SourceSpan};

use super::tests_span_support::{offset_of, parse_err, parse_ok, present};

fn block_source(prefix: &str, kind: &str, name: &str, newline: &str) -> String {
    let body = if kind == "auth" {
        "  id String"
    } else {
        "  provider = \"postgresql\""
    };
    format!("{prefix}  {kind} {name} {{\n{body}\n}}").replace('\n', newline)
}

fn block_name_span(schema: Schema, kind: &str) -> SourceSpan {
    let span = if kind == "auth" {
        schema.auth.map(|block| block.name_span)
    } else {
        schema.datasource.map(|block| block.name_span)
    };
    present(span, &format!("{kind} block"))
}

#[test]
fn valid_blocks_accept_lf_and_crlf_after_preceding_lines() {
    for newline in ["\n", "\r\n"] {
        for prefix in ["", "\n", "transport rest\n// café project schema\n"] {
            for (kind, name) in [("datasource", "db"), ("auth", "UserAuth")] {
                parse_ok(&block_source(prefix, kind, name, newline));
            }
        }
    }
}

#[test]
fn reserved_blocks_keep_exact_spans_with_lf_and_crlf() {
    for newline in ["\n", "\r\n"] {
        for prefix in ["", "\n", "transport rest\n// café project schema\n"] {
            for kind in ["datasource", "auth"] {
                for name in ["part", "import"] {
                    let source = block_source(prefix, kind, name, newline);
                    let error = parse_err(&source);
                    let start = offset_of(&source, &format!("{kind} {name}")) + kind.len() + 1;
                    assert_eq!(error.span(), start..start + name.len(), "{source:?}");
                    let message = format!("{error}");
                    assert!(
                        message.contains("reserved for multi-file schemas"),
                        "{message}"
                    );
                    assert!(message.contains(&format!("`{name}`")), "{message}");
                }
            }
        }
    }
}

#[test]
fn block_name_spans_skip_matching_text_in_the_declaration_keyword() {
    for newline in ["\n", "\r\n"] {
        for (kind, name) in [
            ("datasource", "source"),
            ("datasource", "data"),
            ("datasource", "datasource"),
            ("auth", "auth"),
        ] {
            let source = block_source("// café\n\n", kind, name, newline)
                .replace(&format!("  {kind} "), &format!("\t{kind}   "));
            let span = block_name_span(parse_ok(&source), kind);
            let start = offset_of(&source, &format!("{kind}   {name}")) + kind.len() + 3;
            assert_eq!(span.start..span.end, start..start + name.len());
            assert_eq!(&source[span.start..span.end], name);
            assert_eq!(span.line, 3);
        }
    }
}

#[test]
fn consecutive_blocks_and_fields_have_exact_spans_with_mixed_line_endings() {
    let source = "// café\r\n\ndatasource source {\r\n  provider = \"postgresql\"\n}\r\n\
                  model model {\r\n  id Int @id\n}\r\nauth auth {\r\n  id String\r\n}\n";
    let schema = parse_ok(source);
    let model_name_span = schema.models[0].name_span;
    let datasource = present(schema.datasource, "datasource block");
    let auth = present(schema.auth, "auth block");
    for (span, declaration, name) in [
        (datasource.name_span, "datasource source", "source"),
        (model_name_span, "model model", "model"),
        (auth.name_span, "auth auth", "auth"),
    ] {
        let start = offset_of(source, declaration) + declaration.len() - name.len();
        assert_eq!(span.start..span.end, start..start + name.len());
    }
    for (span, body) in [
        (
            datasource.span,
            "datasource source {\r\n  provider = \"postgresql\"\n}",
        ),
        (auth.span, "auth auth {\r\n  id String\r\n}"),
    ] {
        assert_eq!(&source[span.start..span.end], body);
    }
    let id = &auth.fields[0];
    assert_eq!(&source[id.name_span.start..id.name_span.end], "id");
    assert_eq!(
        &source[id.ty.name_span.start..id.ty.name_span.end],
        "String"
    );
}
