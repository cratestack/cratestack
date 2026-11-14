//! The marked procedures of a schema, and the lookups the views share.
use std::collections::BTreeMap;

use cratestack_core::schema::{DeviceAuthRole, ParsedDeviceAuth, procedure_device_auth};
use cratestack_core::{Field, Procedure, Schema, TypeArity, TypeDecl, TypeRef};

use super::view::{Entry, LegView};
use crate::config::DartGeneratorError;
use crate::idents::{dart_identifier, to_camel_case};
use crate::naming::{occupied_type_names, procedure_wrapper_name};

/// Every `Procedure` the schema marks, by role.
pub(super) struct Legs<'a> {
    pub(super) schema: &'a Schema,
    by_role: BTreeMap<DeviceAuthRole, (&'a Procedure, ParsedDeviceAuth)>,
}

pub(super) fn refuse(why: impl std::fmt::Display) -> DartGeneratorError {
    DartGeneratorError::DeviceAuthSchema(format!(
        "{why}; the parser checks this, so run `cratestack check` on the schema"
    ))
}

impl<'a> Legs<'a> {
    /// `None` when no procedure is marked.
    pub(super) fn of(schema: &'a Schema) -> Result<Option<Self>, DartGeneratorError> {
        let mut by_role = BTreeMap::new();
        for procedure in &schema.procedures {
            if let Some(parsed) = procedure_device_auth(procedure)
                && by_role.insert(parsed.role, (procedure, parsed)).is_some()
            {
                return Err(refuse("a role is marked on two procedures"));
            }
        }
        if by_role.is_empty() {
            return Ok(None);
        }
        for role in DeviceAuthRole::REQUIRED {
            if !by_role.contains_key(&role) {
                return Err(refuse(format!("no procedure has role `{}`", role.name())));
            }
        }
        Ok(Some(Self { schema, by_role }))
    }

    pub(super) fn get(&self, role: DeviceAuthRole) -> Option<&(&'a Procedure, ParsedDeviceAuth)> {
        self.by_role.get(&role)
    }

    pub(super) fn required(&self, role: DeviceAuthRole) -> (&'a Procedure, &ParsedDeviceAuth) {
        let (procedure, parsed) = &self.by_role[&role];
        (procedure, parsed)
    }

    /// The `type` a plain type reference names.
    pub(super) fn decl(&self, ty: &TypeRef) -> Result<&'a TypeDecl, DartGeneratorError> {
        let plain = ty.arity == TypeArity::Required && ty.generic_args.is_empty();
        self.schema
            .types
            .iter()
            .find(|decl| plain && decl.name == ty.name)
            .ok_or_else(|| refuse(format!("`{}` is not a plain `type`", ty.name)))
    }

    pub(super) fn argument(
        &self,
        procedure: &Procedure,
    ) -> Result<&'a TypeDecl, DartGeneratorError> {
        match procedure.args.as_slice() {
            [arg] => self.decl(&arg.ty),
            _ => Err(refuse(format!(
                "procedure `{}` does not take one `type` argument",
                procedure.name
            ))),
        }
    }

    pub(super) fn returns(
        &self,
        procedure: &Procedure,
    ) -> Result<&'a TypeDecl, DartGeneratorError> {
        self.decl(&procedure.return_type)
    }

    /// The call surface of `procedure`, with `args` as its type's arguments.
    pub(super) fn leg(
        &self,
        procedure: &Procedure,
        args: Vec<Entry>,
    ) -> Result<LegView, DartGeneratorError> {
        let argument = self.argument(procedure)?;
        Ok(LegView {
            method: to_camel_case(&procedure.name),
            wrapper: procedure_wrapper_name(procedure, &occupied_type_names(self.schema)),
            arg_field: dart_identifier(&procedure.args[0].name),
            arg_type: argument.name.clone(),
            args,
        })
    }
}

pub(super) fn field<'a>(decl: &'a TypeDecl, name: &str) -> Option<&'a Field> {
    decl.fields.iter().find(|field| field.name == name)
}

pub(super) fn is_optional(field: &Field) -> bool {
    field.ty.arity == TypeArity::Optional
}

/// A `named: expr` argument, the name being the field's Dart identifier.
pub(super) fn entry(field: &Field, expr: String) -> Entry {
    Entry {
        name: dart_identifier(&field.name),
        expr,
    }
}

/// `'wire name'`, the key of a `Map<String, Object?>` element.
pub(super) fn key(field: &Field) -> String {
    format!("'{}'", field.name)
}

/// `answer.<field>`, the Dart read of a generated return type's field.
pub(super) fn answer(field: &Field) -> String {
    format!("answer.{}", dart_identifier(&field.name))
}

/// A map element from `field` of the answer: `'wire': answer.field`, or the
/// null-aware `'wire': ?answer.field` for an optional one.
pub(super) fn answer_element(field: &Field) -> String {
    let null_aware = if is_optional(field) { "?" } else { "" };
    format!("{}: {null_aware}{}", key(field), answer(field))
}

/// A required `String` (`'${...}'`) or an optional one (`?.toString()`) read
/// from `source['wire']`, where `source` is a `Map<String, Object?>`.
pub(super) fn text_from(source: &str, field: &Field) -> String {
    if is_optional(field) {
        format!("{source}['{}']?.toString()", field.name)
    } else {
        format!("'${{{source}['{}'] ?? ''}}'", field.name)
    }
}

/// The same for a lone `Object?` value, the `proof`.
pub(super) fn text_of(value: &str, field: &Field) -> String {
    if is_optional(field) {
        format!("{value}?.toString()")
    } else {
        format!("'${{{value} ?? ''}}'")
    }
}
