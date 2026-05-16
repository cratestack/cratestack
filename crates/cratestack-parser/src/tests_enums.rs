#![cfg(test)]

use super::parse_schema;

#[test]
fn parses_enums_and_allows_enum_type_references() {
    let schema = parse_schema(
        r#"
enum Role {
  admin
  member
}

auth SessionUser {
  role Role
}

model User {
  id Int @id
  role Role
}

type PublicUser {
  role Role
}

procedure getUsers(role: Role?): User
"#,
    )
    .expect("schema with enums should parse");

    assert_eq!(schema.enums.len(), 1);
    assert_eq!(schema.enums[0].name, "Role");
    assert_eq!(
        schema.enums[0]
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        vec!["admin", "member"]
    );
    assert_eq!(
        schema.auth.as_ref().expect("auth block").fields[0].ty.name,
        "Role"
    );
    assert_eq!(schema.models[0].fields[1].ty.name, "Role");
    assert_eq!(schema.types[0].fields[0].ty.name, "Role");
    assert_eq!(schema.procedures[0].args[0].ty.name, "Role");
}

#[test]
fn rejects_duplicate_enum_variants() {
    let error = parse_schema(
        r#"
enum Role {
  admin
  admin
}
"#,
    )
    .expect_err("duplicate enum variants should fail validation");

    assert!(
        error
            .to_string()
            .contains("duplicate variant `admin` on enum `Role`")
    );
}
