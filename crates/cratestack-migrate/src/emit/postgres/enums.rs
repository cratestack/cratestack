//! Enum-type DDL: CREATE TYPE / ALTER TYPE ADD VALUE / DROP TYPE.

use std::fmt::Write as _;

use crate::ir::{AlterEnumAddVariant, CreateEnum, DropEnum};
use crate::naming;

use super::idents::quote_ident;

pub(super) fn emit_create_enum(sql: &mut String, create: &CreateEnum) {
    let type_name = quote_ident(&naming::column_name(&create.name));
    let variants: Vec<String> = create
        .variants
        .iter()
        .map(|v| format!("'{}'", v.replace('\'', "''")))
        .collect();
    writeln!(
        sql,
        "CREATE TYPE {type_name} AS ENUM ({});",
        variants.join(", ")
    )
    .unwrap();
}

pub(super) fn emit_alter_enum_add(sql: &mut String, alter: &AlterEnumAddVariant) {
    let type_name = quote_ident(&naming::column_name(&alter.name));
    writeln!(
        sql,
        "ALTER TYPE {type_name} ADD VALUE '{}';",
        alter.value.replace('\'', "''")
    )
    .unwrap();
}

pub(super) fn emit_drop_enum(sql: &mut String, drop: &DropEnum) {
    let type_name = quote_ident(&naming::column_name(&drop.name));
    writeln!(sql, "DROP TYPE {type_name};").unwrap();
}
