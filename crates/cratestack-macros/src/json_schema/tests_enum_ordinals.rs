//! Enums in a tool's JSON Schema. A string enum is its names; an ordinal
//! enum (`Pending = 1`, `docs/design/enum-ordinals.md` §2.2) is the
//! integer it is sent as, each option titled with its variant's name so an
//! agent reading the schema still sees the names.

use serde_json::json;

use super::procedure_input_schema;
use super::tests::parse;

const SOURCE: &str = "enum Status {\n  Pending = 1\n  Refunded = 5\n  @@reserved(2)\n}\n\
    enum Role {\n  admin\n  member\n}\n\
    procedure check(status: Status, role: Role): Boolean\n";

#[test]
fn an_ordinal_enum_is_an_integer_titled_by_name_and_a_string_enum_its_names() {
    let schema = parse(SOURCE);
    let input = procedure_input_schema(&schema, &schema.procedures[0], None).expect("input schema");
    assert_eq!(
        input["properties"]["status"],
        json!({ "$ref": "#/$defs/Status" })
    );
    assert_eq!(
        input["$defs"]["Status"],
        json!({
            "type": "integer",
            "oneOf": [
                { "const": 1, "title": "Pending" },
                { "const": 5, "title": "Refunded" },
            ],
        })
    );
    assert_eq!(
        input["$defs"]["Role"],
        json!({ "type": "string", "enum": ["admin", "member"] })
    );
}
