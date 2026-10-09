//! The closed lists of field attributes, one per declaration kind that has
//! fields (ADR 0019 D5, superseding cratestack#679's option (b)).
//!
//! A field attribute is parsed as opaque text and each reader matches the
//! text itself, so a name no reader knows had no effect at all: `@string`,
//! `@wire(string)` and `@immutable` passed `cratestack check`, and the
//! protection the author believed they had written was absent. A field of a
//! `model`, `view`, `mixin`, `type` or the `auth` block now accepts only the
//! names below for its kind, checked by `super::attribute_shape::check_shape`.
//!
//! The union is 19 names. Each kind's list holds the names some reader
//! reads on a field of that kind (paths under `crates/`; `shared/attrs.rs`
//! is `cratestack-macros/src/shared/attrs.rs`):
//!
//! | Attribute | Arguments | Readers |
//! |-----------|-----------|---------|
//! | `@id` | none | model, view: `cratestack-core/src/schema/attribute_text_names.rs:38` through `Field::is_primary_key`; model `cratestack-macros/src/model/descriptor.rs:39`, `cratestack-migrate/src/convert/fields.rs:101`; view `cratestack-macros/src/view/descriptor.rs:49`, `view/accessor.rs:36`, `cratestack-migrate/src/diff/views.rs:102` |
//! | `@unique` | none | model: `cratestack-migrate/src/convert/fields.rs:104` |
//! | `@default` | required | model: `shared/attrs.rs:69`, `:76`, `:88`, `cratestack-migrate/src/convert/fields.rs:126`, `cratestack-client-typescript/src/types.rs:174`, `cratestack-client-dart/src/naming.rs:96`; not a `type` field, see below |
//! | `@relation` | required | model: `cratestack-macros/src/relation/parse.rs:10`, `cratestack-migrate/src/convert/relations.rs:33`, `fields.rs:111` |
//! | `@computed` | optional (`@computed(params: <Type>?)`) | model, type: `cratestack-core/src/schema/computed_attribute.rs:81`, `cratestack-macros/src/types.rs:36`, `shared.rs:109` |
//! | `@readonly` | none | model: `shared/attrs.rs:38`, `model/inputs.rs:23`, `:31`, `model/descriptor/columns.rs:94` |
//! | `@server_only` | none | model, view: `shared/attrs.rs:46`, `model/struct_only/field_definition.rs:71` (a view's struct is emitted through it, `view/struct_only.rs:27`); model also `cratestack-client-typescript/src/types.rs:140` |
//! | `@version` | none | model: `shared/attrs.rs:93`, `model/descriptor.rs:169`, `axum/model/prep.rs:80` |
//! | `@pii`, `@sensitive` | none | model: `shared/attrs.rs:54`, `:62`, `model/descriptor/columns.rs:66`, `:74` (audit redaction) |
//! | `@db_enforce` | none | model: `cratestack-migrate/src/convert/checks.rs:12` |
//! | `@email`, `@uri`, `@iso4217` | none | model: `cratestack-macros/src/validators.rs:105` to `:107`, `cratestack-studio/src/validators/predicates.rs`; `@iso4217` also `cratestack-migrate/src/convert/checks.rs:31`; type: `cratestack-macros/src/validators/types.rs:37` through the same `validators.rs` arms |
//! | `@length`, `@range`, `@regex` | required | model: `cratestack-macros/src/validators.rs:90` to `:104`; `@length` and `@range` also `cratestack-migrate/src/convert/checks.rs:25`; type: `cratestack-macros/src/validators/types.rs:37`, through the same arms |
//! | `@rename` | required, exactly `from = "<old>"` (`super::rename_attributes`) | model: `cratestack-migrate/src/convert/renames.rs:25` |
//! | `@from` | required | view: none reads it; see below |
//!
//! A mixin's fields are copied into every model that `@use`s it, attributes
//! included (`crate::parse::models::expand_model_mixins`), so a mixin
//! accepts what a model accepts, minus the two names its dedicated checks
//! refuse (`@id`, `super::mixins_types`; `@computed`,
//! `super::computed_attribute`). An `auth` field accepts nothing: the macros
//! read only its name (`cratestack-macros/src/policy/auth.rs:11`), and
//! `@server_only` and `@computed` on it already had refusals of their own.
//!
//! One entry is accepted although nothing reads it, because the
//! documentation and the ADR say it is written there, and refusing it is a
//! decision for the maintainer: `@from(Model.field)` on a view field is
//! documented as an unchecked source annotation (`cratestack-docs`
//! `reference/views.md`), is named by ADR 0019 D5, and appears 11 times in
//! this repository.
//!
//! A `type` field is the case the first cut of these lists got wrong, and the
//! reason there is no such entry for it. It listed `@default` and `@length`
//! because committed schemas wrote them, although neither did anything: a
//! `type` is decoded as sent, so `@default` is never applied
//! (`cratestack-core/src/client_contract/compat_decl.rs:55`, pinned by
//! `cratestack-api/tests/contract_roundtrip.rs`), and only a model's create
//! and update inputs ran validators. Accepting an attribute that does nothing
//! is what D5 closes, and downstream schemas (skyport-billing, the vaam p2p
//! and mobile-v3 schemas) wrote `@length` on `type`s used as procedure
//! arguments believing it checked them. So the validator family is accepted
//! on a `type` field and enforced on procedure arguments
//! (`cratestack-macros/src/validators/types.rs`), and the two names with no
//! reader are refused, each with its own message
//! (`super::type_field_attributes`): `@default`, and `@db_enforce`, since a
//! `type` has no table. A validator on a list field is refused there too.
//!
//! The other attributes, `@pii`, `@unique` and the rest, are refused on a
//! `type` or `view` field, where they were inert.

use super::attribute_shape::{Arguments, Known};

pub(super) const MODEL_FIELD_ATTRIBUTES: &[Known] = &[
    ("@id", Arguments::None),
    ("@unique", Arguments::None),
    ("@default", Arguments::Required),
    ("@relation", Arguments::Required),
    ("@computed", Arguments::Optional),
    ("@readonly", Arguments::None),
    ("@server_only", Arguments::None),
    ("@version", Arguments::None),
    ("@pii", Arguments::None),
    ("@sensitive", Arguments::None),
    ("@db_enforce", Arguments::None),
    ("@email", Arguments::None),
    ("@uri", Arguments::None),
    ("@iso4217", Arguments::None),
    ("@length", Arguments::Required),
    ("@range", Arguments::Required),
    ("@regex", Arguments::Required),
    ("@rename", Arguments::Required),
];

pub(super) const VIEW_FIELD_ATTRIBUTES: &[Known] = &[
    ("@id", Arguments::None),
    ("@server_only", Arguments::None),
    ("@from", Arguments::Required),
];

pub(super) const MIXIN_FIELD_ATTRIBUTES: &[Known] = &[
    ("@unique", Arguments::None),
    ("@default", Arguments::Required),
    ("@relation", Arguments::Required),
    ("@readonly", Arguments::None),
    ("@server_only", Arguments::None),
    ("@version", Arguments::None),
    ("@pii", Arguments::None),
    ("@sensitive", Arguments::None),
    ("@db_enforce", Arguments::None),
    ("@email", Arguments::None),
    ("@uri", Arguments::None),
    ("@iso4217", Arguments::None),
    ("@length", Arguments::Required),
    ("@range", Arguments::Required),
    ("@regex", Arguments::Required),
    ("@rename", Arguments::Required),
];

pub(super) const TYPE_FIELD_ATTRIBUTES: &[Known] = &[
    ("@computed", Arguments::Optional),
    ("@email", Arguments::None),
    ("@uri", Arguments::None),
    ("@iso4217", Arguments::None),
    ("@length", Arguments::Required),
    ("@range", Arguments::Required),
    ("@regex", Arguments::Required),
];

pub(super) const AUTH_FIELD_ATTRIBUTES: &[Known] = &[];
