//! Reject schema names that collide with a *generated* `{Name}Builder`
//! typestate-builder symbol (see `cratestack-macros/src/builder.rs`).
//!
//! Every struct-shaped generated type now emits `impl {Name} {
//! fn builder() }` plus a `{Name}Builder` struct into the *same module* the
//! `{Name}` struct itself lives in (`types`/`models` — see the module docs on
//! [`super::snake_case_collisions::validate_type_declaration_collisions`]
//! for why `type`/`enum`/`model` already share one generated namespace, and
//! `view` structs are emitted straight into the `models` module alongside
//! model structs). If a schema *also* declares an entity literally named
//! `{Name}Builder`, the generated builder and the user's own declaration
//! land on the same identifier:
//!
//! - Same-module case (`type Foo` + `type FooBuilder`, or `model Foo` +
//!   `model FooBuilder`): `error[E0428]: the name `FooBuilder` is defined
//!   multiple times`, reported only at the `include_*_schema!` call site.
//! - Cross-module case (`type Order` + `model OrderBuilder`): both
//!   `types::OrderBuilder` (the generated builder for `type Order`) and
//!   `models::OrderBuilder` (the struct for `model OrderBuilder`) reach the
//!   parent module through `pub use types::*; pub use models::*;` —
//!   `error[E0659]: `OrderBuilder` is ambiguous`, silent until something
//!   actually names the type.
//!
//! Both are opaque, hard-to-diagnose failures at macro-expansion time, so —
//! same rationale as [`super::reserved_idents`] — reject the collision at
//! schema-parse time with a message that names the two conflicting
//! declarations directly.
//!
//! A `model` generates *seven* struct-shaped names across the two
//! generators that currently emit builders (Rust and Dart — see
//! `cratestack-macros/src/builder.rs`'s module doc for the full list of
//! call sites): `{M}`, `Create{M}Input`, `Update{M}Input`, `{M}Where`,
//! `{M}OrderByClause`, `{M}FindManyInput` (the Rust struct name,
//! `cratestack-macros/src/model/find_many_input.rs`), and `{M}FindMany`
//! (the *Dart* class name for that same generated shape,
//! `cratestack-client-dart/src/find_many_views.rs`'s
//! `build_find_many_data_class` — Dart drops the `Input` suffix). Each
//! claims a `{...}Builder` name, so all seven are reserved. (`{M}SortField`
//! is an enum and generates no builder.) The builder's own value holder is
//! an anonymous tuple, deliberately, so it claims no name here at all —
//! see `cratestack-macros/src/builder.rs`.
//!
//! A `procedure` generates one more: its Dart-side argument wrapper class,
//! `{PascalCase(name)}Args` (`cratestack-client-dart/src/naming.rs`'s
//! `procedure_wrapper_name`), together with the two names that function
//! falls back to when the default is taken — all three are reserved; see
//! the `Kind::Procedure` doc. Note a procedure is a collision *source*
//! only, never a target: it generates `{P}Args`, but nothing named `{P}`
//! itself, so `procedure WidgetBuilder(..)` alongside `model Widget` is
//! valid and must not be rejected. The equivalent Rust struct is scoped inside
//! `pub mod <procedure>` on both the server and the Rust client
//! (`cratestack-macros/src/procedure/types.rs`), so it never collides with
//! anything at the shared `types`/`models` namespace this validator
//! covers and needs no reservation.
//!
//! Comparison is exact raw-name equality, not [`to_snake_case`] normalized:
//! `format_ident!("{}Builder", target)` is a literal string concatenation
//! on the already-validated (schema-representable) declared name, with no
//! case folding involved. The one exception is the procedure-name ->
//! PascalCase step itself, which uses the single shared
//! [`cratestack_core::pascal_case::to_pascal_case`] — the same function
//! `cratestack-client-dart` calls to build the real symbol — rather than a
//! second copy here that could drift from it.
//!
//! [`to_snake_case`]: cratestack_core::route_naming::to_snake_case

use cratestack_core::pascal_case::to_pascal_case;
use cratestack_core::{Schema, SourceSpan};

use crate::diagnostics::{SchemaError, span_error};

/// One declared name that participates in the shared `types`/`models`
/// generated-symbol namespace, with enough metadata to report a collision.
struct Entry<'a> {
    name: &'a str,
    span: SourceSpan,
    kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Type,
    Enum,
    Model,
    View,
    /// A `procedure` declaration — a collision *source* only (see
    /// [`validate_builder_name_collisions`]). Its Dart-side argument
    /// wrapper class is reserved under every name
    /// `cratestack-client-dart`'s `procedure_wrapper_name` can produce, not
    /// just the default; see [`Kind::generated_struct_names`] for why all
    /// three rungs of that fallback chain are covered.
    Procedure,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Type => "type",
            Kind::Enum => "enum",
            Kind::Model => "model",
            Kind::View => "view",
            Kind::Procedure => "procedure",
        }
    }

    /// Every struct-shaped type this declaration causes the macros to
    /// emit — each of which gets its own `{Name}Builder`.
    ///
    /// A `model` is the interesting case: it emits far more than its own
    /// struct, and each derived struct claims a builder name too. Checking
    /// only the declared name (the shape this validator originally
    /// shipped with) left `type CreateTaskInputBuilder` alongside
    /// `model Task` passing `cratestack check` and then failing as
    /// `error[E0659]: \`CreateTaskInputBuilder\` is ambiguous` at the
    /// consumer's use site — silent until something actually names the
    /// type, which is the worst possible time to find out.
    ///
    /// `enum` emits no builder (not struct-shaped), and neither does
    /// `{Model}SortField`, which is likewise an enum.
    fn generated_struct_names(self, name: &str) -> Vec<String> {
        match self {
            Kind::Type | Kind::View => vec![name.to_owned()],
            Kind::Model => vec![
                name.to_owned(),
                format!("Create{name}Input"),
                format!("Update{name}Input"),
                format!("{name}Where"),
                format!("{name}OrderByClause"),
                format!("{name}FindManyInput"),
                format!("{name}FindMany"),
            ],
            Kind::Enum => Vec::new(),
            // All three spellings `procedure_wrapper_name`
            // (`cratestack-client-dart/src/naming.rs`) can land on, not just
            // the first. Its fallback chain is
            // `{P}Args` -> `{P}ProcedureArgs` -> `{P}ProcedureRequest`, and a
            // previous revision reserved only the first — so a schema that
            // pushed the generator onto rung two still emitted two Dart
            // classes named `{P}ProcedureArgsBuilder` while `cratestack
            // check` said "schema OK".
            //
            // Reserving all three unconditionally over-reserves by two names
            // when the fallback isn't triggered. That is deliberate: the
            // alternative is replicating naming.rs's occupied-name chain
            // here, in a crate that cannot depend on the Dart generator, and
            // the two would silently drift apart the moment that chain
            // changes. A schema declaring a type named literally
            // `{P}ProcedureRequestBuilder` gets a clear rename message it can
            // act on; the drift failure mode is opaque generated-code
            // breakage, which is strictly worse.
            Kind::Procedure => {
                let pascal = to_pascal_case(name);
                vec![
                    format!("{pascal}Args"),
                    format!("{pascal}ProcedureArgs"),
                    format!("{pascal}ProcedureRequest"),
                ]
            }
        }
    }
}

/// Reject any declared `type`/`model`/`view`/`enum` name that exactly
/// matches the generated `{Name}Builder` symbol of another `type`/`model`/
/// `view` declaration in the same schema.
pub(super) fn validate_builder_name_collisions(schema: &Schema) -> Result<(), SchemaError> {
    let mut entries: Vec<Entry<'_>> = Vec::new();

    for ty in &schema.types {
        entries.push(Entry {
            name: ty.name.as_str(),
            span: ty.span,
            kind: Kind::Type,
        });
    }
    for enum_decl in &schema.enums {
        entries.push(Entry {
            name: enum_decl.name.as_str(),
            span: enum_decl.span,
            kind: Kind::Enum,
        });
    }
    for model in &schema.models {
        entries.push(Entry {
            name: model.name.as_str(),
            span: model.span,
            kind: Kind::Model,
        });
    }
    for view in &schema.views {
        entries.push(Entry {
            name: view.name.as_str(),
            span: view.name_span,
            kind: Kind::View,
        });
    }
    // Procedures are collision *sources* only, never targets. A procedure
    // generates `{P}Args` (and so `{P}ArgsBuilder`), but nothing named `{P}`
    // itself — so `procedure WidgetBuilder(..)` alongside `model Widget` is
    // perfectly fine, and an earlier revision of this check rejected it
    // because procedures had been added to the one shared list. Hence two
    // lists rather than one.
    let sources = entries
        .iter()
        .map(|entry| (entry.kind, entry.name))
        .chain(
            schema
                .procedures
                .iter()
                .map(|procedure| (Kind::Procedure, procedure.name.as_str())),
        )
        .collect::<Vec<_>>();

    for (source_kind, source_name) in sources {
        for generated in source_kind.generated_struct_names(source_name) {
            let builder_name = format!("{generated}Builder");
            // No entry can be its own collision target: that would need
            // `name == <something ending in that name> + "Builder"`, which
            // no finite string satisfies.
            let Some(target) = entries.iter().find(|entry| entry.name == builder_name) else {
                continue;
            };
            // Name the *generated* struct when it isn't the declaration
            // itself — "collides with the builder for `CreateTaskInput`"
            // is actionable in a way that "collides with something `Task`
            // emits" is not.
            let origin = if generated == source_name {
                format!("{} `{}`", source_kind.label(), source_name)
            } else {
                format!(
                    "`{generated}`, which {} `{}` generates",
                    source_kind.label(),
                    source_name
                )
            };
            return Err(span_error(
                format!(
                    "{} `{}` collides with the typestate builder generated for {origin} — \
                     that builder is a struct literally named `{builder_name}` in the same \
                     generated namespace; rename one of them",
                    target.kind.label(),
                    target.name,
                ),
                target.span,
            ));
        }
    }

    Ok(())
}
