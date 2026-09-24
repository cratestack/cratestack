//! Walks the `.cstack` IR the same way codegen does and emits the schema
//! of what serde sees. Each branch cites the codegen it mirrors, because
//! the schema is only right while the two agree.

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Field, Schema, TypeArity, TypeRef};
use serde_json::{Map, Value, json};

use super::error::JsonSchemaError;
use super::parts::{is_required, nullable, object_schema, page, page_input, with_docs};
use super::scalar::{Scalar, builtin_scalar};
use crate::shared::decimal_backend::DecimalBackend;
use crate::shared::{is_computed_field, is_relation_field, is_server_only_field};

pub(super) struct Generator<'a> {
    schema: &'a Schema,
    decimal: Option<DecimalBackend>,
    model_names: BTreeSet<&'a str>,
    /// Named declarations, emitted once under `$defs` and referenced by
    /// `$ref`. That keeps a type used twice from being inlined twice, and
    /// it is what lets a self-referencing `type` terminate.
    defs: BTreeMap<String, Value>,
}

impl<'a> Generator<'a> {
    pub(super) fn new(schema: &'a Schema, decimal: Option<DecimalBackend>) -> Self {
        Self {
            schema,
            decimal,
            model_names: schema.models.iter().map(|m| m.name.as_str()).collect(),
            defs: BTreeMap::new(),
        }
    }

    /// Adds the dialect and the collected `$defs` to a root schema.
    pub(super) fn finish(self, mut root: Value) -> Value {
        if let Some(object) = root.as_object_mut() {
            object.insert("$schema".to_owned(), json!(super::DIALECT));
            if !self.defs.is_empty() {
                let defs = self.defs.into_iter().collect::<Map<_, _>>();
                object.insert("$defs".to_owned(), Value::Object(defs));
            }
        }
        root
    }

    /// One argument's or field's schema, arity included. `Page<T>` and
    /// `FindMany<T>` ignore arity, as `procedure_type_tokens` does: it
    /// returns before wrapping them in `Option`/`Vec`.
    pub(super) fn type_ref(&mut self, ty: &TypeRef) -> Result<Value, JsonSchemaError> {
        if ty.is_page() {
            return self.page(ty);
        }
        if ty.is_find_many() {
            return Err(JsonSchemaError::NoFaithfulMapping {
                type_name: "FindMany".to_owned(),
                reason: "it deserializes into a generated `<Model>FindManyInput`, whose filter \
                         and ordering grammar has no JSON Schema mapping yet",
                at: Vec::new(),
            });
        }
        let element = self.element(ty)?;
        Ok(match ty.arity {
            TypeArity::Required => element,
            TypeArity::Optional => nullable(element),
            TypeArity::List => json!({ "type": "array", "items": element }),
        })
    }

    fn element(&mut self, ty: &TypeRef) -> Result<Value, JsonSchemaError> {
        if ty.is_page_input() {
            return Ok(page_input());
        }
        match builtin_scalar(&ty.name, self.decimal) {
            Some(Scalar::Mapped(schema)) => Ok(schema),
            Some(Scalar::NoFaithfulMapping(reason)) => Err(JsonSchemaError::NoFaithfulMapping {
                type_name: ty.name.clone(),
                reason,
                at: Vec::new(),
            }),
            Some(Scalar::NeedsDecimalBackend) => {
                Err(JsonSchemaError::MissingDecimalBackend { at: Vec::new() })
            }
            None => self.declaration(&ty.name),
        }
    }

    /// `Page<T>`'s item, wrapped in the fixed envelope from `parts.rs`.
    fn page(&mut self, ty: &TypeRef) -> Result<Value, JsonSchemaError> {
        let Some(item) = ty.page_item() else {
            return Err(JsonSchemaError::UnknownType {
                type_name: "Page".to_owned(),
                at: Vec::new(),
            });
        };
        let items = self.type_ref(item).map_err(|e| e.within("`Page` item"))?;
        Ok(page(items))
    }

    fn declaration(&mut self, name: &str) -> Result<Value, JsonSchemaError> {
        let reference = json!({ "$ref": format!("#/$defs/{name}") });
        if self.defs.contains_key(name) {
            return Ok(reference);
        }
        // Placeholder first, so a type that refers to itself finds its
        // own entry and emits a `$ref` instead of recursing forever.
        self.defs.insert(name.to_owned(), Value::Null);
        let schema = self.declaration_schema(name)?;
        self.defs.insert(name.to_owned(), schema);
        Ok(reference)
    }

    fn declaration_schema(&mut self, name: &str) -> Result<Value, JsonSchemaError> {
        let schema = self.schema;
        if let Some(decl) = schema.enums.iter().find(|e| e.name == name) {
            // Every variant carries `#[serde(rename = "<Variant>")]`
            // (`crate::types::enums::variant_tokens`).
            let variants: Vec<&str> = decl.variants.iter().map(|v| v.name.as_str()).collect();
            let enum_schema = json!({ "type": "string", "enum": variants });
            return Ok(with_docs(enum_schema, &decl.docs));
        }
        if let Some(decl) = schema.types.iter().find(|t| t.name == name) {
            // `crate::types::generate_type_struct`: every field, no serde
            // attributes beyond `Bytes`' lenient deserializer.
            reject_computed(name, &decl.fields)?;
            let object = self.object(name, decl.fields.iter().collect())?;
            return Ok(with_docs(object, &decl.docs));
        }
        if let Some(model) = schema.models.iter().find(|m| m.name == name) {
            // `crate::model::generate_model_struct_only`: relation fields
            // are not struct fields at all. `@server_only` fields are serde
            // `skip` (cratestack#1051): never written, never read. They are
            // left out rather than advertised to an agent.
            reject_computed(name, &model.fields)?;
            let fields = model
                .fields
                .iter()
                .filter(|f| !is_relation_field(&self.model_names, f) && !is_server_only_field(f))
                .collect();
            let object = self.object(name, fields)?;
            return Ok(with_docs(object, &model.docs));
        }
        Err(JsonSchemaError::UnknownType {
            type_name: name.to_owned(),
            at: Vec::new(),
        })
    }

    fn object(&mut self, owner: &str, fields: Vec<&Field>) -> Result<Value, JsonSchemaError> {
        let mut properties = Map::new();
        let mut required = Vec::new();
        for field in fields {
            let property = self
                .type_ref(&field.ty)
                .map_err(|e| e.within(format!("field `{owner}.{}`", field.name)))?;
            properties.insert(field.name.clone(), with_docs(property, &field.docs));
            if is_required(&field.ty) {
                required.push(field.name.as_str());
            }
        }
        Ok(object_schema(properties, required))
    }
}

/// A `type` or `model` with `@computed` fields has two serde shapes: the
/// server struct without them (`Output`), and the wire struct with them
/// (`crate::computed::wire`) that REST and RPC fill by composing resolved
/// values in. Which of the two an MCP tool returns depends on whether
/// phase 3's dispatch composes, so no schema is guessed here. (The parser
/// already rejects a computed-bearing type as procedure input.)
fn reject_computed(owner: &str, fields: &[Field]) -> Result<(), JsonSchemaError> {
    match fields.iter().find(|f| is_computed_field(f)) {
        None => Ok(()),
        Some(field) => Err(JsonSchemaError::NoFaithfulMapping {
            type_name: owner.to_owned(),
            reason: "it has `@computed` fields, whose values reach the wire only through \
                     response composition, which MCP dispatch does not do yet",
            at: vec![format!("field `{owner}.{}`", field.name)],
        }),
    }
}
