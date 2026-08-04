//! Renders a single proto field line — the universal presence rule from
//! `docs/design/protobuf.md` §4.4: every non-repeated field is `optional`,
//! `repeated` fields never are (proto3 forbids combining them).

use cratestack_core::{TypeArity, TypeRef};

use super::scalar::map_scalar;

pub(super) struct RenderedField {
    pub(super) line: String,
    pub(super) needs_timestamp_import: bool,
}

pub(super) fn render_field(name: &str, ty: &TypeRef, number: i32) -> RenderedField {
    let mapped = map_scalar(&ty.name);
    let presence = if ty.arity == TypeArity::List {
        "repeated"
    } else {
        "optional"
    };
    let mut line = format!("{presence} {} {name} = {number};", mapped.proto_type);
    if let Some(comment) = mapped.trailing_comment {
        line.push_str(&format!(" // {comment}"));
    }
    RenderedField {
        line,
        needs_timestamp_import: mapped.needs_timestamp_import,
    }
}

#[cfg(test)]
mod tests {
    use cratestack_core::SourceSpan;

    use super::*;

    fn span() -> SourceSpan {
        SourceSpan {
            start: 0,
            end: 0,
            line: 0,
        }
    }

    fn ty(name: &str, arity: TypeArity) -> TypeRef {
        TypeRef {
            name: name.to_owned(),
            name_span: span(),
            arity,
            generic_args: vec![],
            int_args: Vec::new(),
        }
    }

    #[test]
    fn required_scalar_is_optional_in_proto() {
        let rendered = render_field("email", &ty("String", TypeArity::Required), 2);
        assert_eq!(rendered.line, "optional string email = 2;");
    }

    #[test]
    fn optional_scalar_is_still_just_optional() {
        let rendered = render_field("nickname", &ty("String", TypeArity::Optional), 3);
        assert_eq!(rendered.line, "optional string nickname = 3;");
    }

    #[test]
    fn list_field_is_repeated_never_optional() {
        let rendered = render_field("tags", &ty("String", TypeArity::List), 4);
        assert_eq!(rendered.line, "repeated string tags = 4;");
    }

    #[test]
    fn json_field_carries_trailing_comment() {
        let rendered = render_field("meta", &ty("Json", TypeArity::Required), 5);
        assert_eq!(rendered.line, "optional bytes meta = 5; // json");
    }

    #[test]
    fn datetime_field_flags_timestamp_import() {
        let rendered = render_field("createdAt", &ty("DateTime", TypeArity::Required), 1);
        assert_eq!(
            rendered.line,
            "optional google.protobuf.Timestamp createdAt = 1;"
        );
        assert!(rendered.needs_timestamp_import);
    }
}
