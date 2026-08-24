# Computed fields (`@computed`) — resolver-backed response-time fields

Status: in implementation. Source of truth for the `@computed` feature.

## Problem

A schema author wants fields that are *derived at response time* rather than stored —
e.g. a signed `proxyUrl` on an `Image` — computed by hand-written Rust the framework
invokes while composing the response:

```text
model Image {
  id Int @id
  storageKey String
  proxyUrl String @computed
}

type Thumbnail {
  storageKey String
  url String @computed(params: ProxyParams?)
}

type ProxyParams {
  width Int?
  height Int?
}
```

The pre-existing `@custom` attribute (type-only) generated a `CustomFieldResolver`
trait that **nothing ever invoked** — the field stayed a plain struct field the author
had to fill by hand. `@computed` replaces it (parse error on `@custom` points here).

## Decisions (maintainer-confirmed)

1. `@custom` is **removed**, replaced by `@computed`. One concept.
2. Model computed fields resolve on **all model HTTP responses**: get, list,
   create, update/upsert, delete, and relation includes. Event/stream payloads are
   excluded in v1 (the server-side model struct simply doesn't carry the field).
3. `include_embedded_schema!` **rejects** schemas containing `@computed` at macro
   expansion (compile error) — embedded has no response-composition boundary.
4. Resolvers reach the router as a **new `router()` parameter**, always present:
   `router(db, registry, resolvers, codec, auth_provider, body_limit_bytes)`.
   When the schema has no computed fields the generated `ComputedFieldResolver`
   trait has no methods and a generated `impl ComputedFieldResolver for ()` lets
   callers pass `()`.

## Schema surface

- `@computed` — bare marker on a `type` or `model` field.
- `@computed(params: <Type>?)` — parameterized resolver. `<Type>` must be a declared
  `type` (not a model, not a scalar, not computed-bearing). The trailing `?` is
  **required in v1**: params are always optional (a required param would make plain
  CRUD reads unsatisfiable and has no wire slot on non-read paths). `@computed(params:
  <Type>)` without `?` → parse error “required computed params are not supported yet”.
- Accepted on `type` and `model` fields only. Rejected (with spanned errors) on
  mixins, views, auth blocks (`validate::fields::validate_computed_field_attribute`),
  and `@custom` everywhere (`validate::removed_attributes`).
- `@computed` cannot combine with any other field attribute (fail-closed).
- A computed field's own type must be a scalar, enum, or non-computed-bearing `type`;
  never a model.
- **Computed-bearing** (has a computed field, transitively through nested `type`
  fields — see `validate::computed::computed_bearing_names`): such names are rejected
  as procedure *argument* types (the client wire shape includes computed fields, the
  server shape doesn't, so inputs would silently drop data) and as `@stream` item
  types (no item-wise resolution inside the incremental encoder in v1).
- Composite `@@id`/`@@unique`/`@@index` field lists reject computed names.

## Generated server surface

Server-side structs (`models::*`, `types::*`) **exclude** computed fields — they are
never stored, fetched, or hand-constructed. Client-side shapes (generated Rust client
via `include_client_schema!`, Dart, TypeScript) **include** them (that is the wire
shape) but exclude them from create/update inputs, filters, and sorts.

Per schema, the macro emits a `computed` module:

```rust
pub mod computed {
    pub struct ComputedFieldDescriptor {
        pub owner: &'static str,          // "Image"
        pub field: &'static str,          // "proxyUrl"
        pub resolver_method: &'static str, // "resolve_image_proxy_url"
        pub params_type: Option<&'static str>, // Some("ProxyParams")
    }
    pub const FIELDS: &[ComputedFieldDescriptor] = &[ ... ];

    pub trait ComputedFieldResolver: Clone + Send + Sync + 'static {
        // without params:
        fn resolve_image_proxy_url(
            &self,
            db: &super::Cratestack,
            source: &super::Image,
            ctx: &::cratestack::CratestackContext,
        ) -> impl Future<Output = Result<String, CratestackError>> + Send;
        // with `@computed(params: ProxyParams?)`:
        fn resolve_thumbnail_url(
            &self,
            db: &super::Cratestack,
            source: &super::Thumbnail,
            params: Option<&super::ProxyParams>,
            ctx: &::cratestack::CratestackContext,
        ) -> impl Future<Output = Result<String, CratestackError>> + Send;
    }
    // Only when FIELDS is empty:
    impl ComputedFieldResolver for () {}
}
```

`source` is the *server-side* struct (computed fields excluded). Method naming:
`resolve_<owner_snake>_<field_snake>`.

## Response composition

### Models (Postgres server schemas)

`serialize_<model>_model_value` (the shared GET/list/include projection path) gains
`resolvers: &CR` and, after projecting stored fields, resolves and inserts each
computed field as a `ProjectedValue::leaf`. Selection semantics: with no `?fields=`,
every computed field resolves; with `?fields=`, only selected ones resolve (computed
names are legal in `allowed_fields` but never in sorts/filters).

Create/update/delete handlers today encode the struct directly. For models **with**
computed fields they switch to the projection serializer (full default selection) so
the wire shape includes computed values; models without computed fields keep the
existing direct encode, bit-identical.

`ModelRouterState` gains the resolver: `ModelRouterState<CR, C, Auth>`; `router()` and
`model_router()` thread it through. RPC dispatch reuses the same `*_dispatch`
functions and inherits the behavior.

### Procedure outputs

For every computed-bearing owner (type or model) the macro emits an async
`compose_<owner_snake>` helper turning `&T` into a `ProjectedValue` (stored fields as
leaves, computed fields resolved, nested computed-bearing `type`/model fields recursed
through their own compose helpers, `Option`/`Vec` arities handled). Procedures whose
`Output` (or `Page<T>` item / list item) is computed-bearing compose before encoding;
all other procedures encode exactly as today. `ProcedureRouterState` gains `CR` the
same way. Procedure-context resolution always passes `params: None` in v1.

### Parameterized resolvers on the wire

Model GET/list REST requests accept one query parameter:

```
?computedParams=%7B%22proxyUrl%22%3A%7B%22width%22%3A800%7D%7D
   (URL-encoded {"proxyUrl": {"width": 800}})
```

- JSON object keyed by computed field name; each value deserializes into the
  generated params `type` struct via serde. Malformed JSON, unknown field keys,
  keys naming non-computed or param-less fields, or a params payload for a field
  excluded by `?fields=` → `CratestackError::Validation`.
- Absent `computedParams` (or absent key) → resolver gets `None`.
- Applies to the request's *root* model only in v1; relation-included records and
  all non-read paths resolve with `None`.
- Generated clients (Rust/Dart/TS) expose typed optional params on get/list calls
  and serialize them into this query parameter.

## Exclusions (v1, documented)

- Event/change-stream payloads never carry computed fields.
- `@stream` procedures with computed-bearing items: parse error.
- Embedded (`include_embedded_schema!`): compile error when the schema has any
  computed field.
- Views cannot declare computed fields.
- Audit-log redaction (`@pii`/`@sensitive`) doesn't apply (cannot combine with
  `@computed`); resolvers must not return data needing redaction.

## Downstream

- `cratestack-migrate`: computed fields excluded from DDL/diff.
- Wiremock generator: computed fields fabricated like ordinary response fields.
- Dart/TS clients: computed fields in response classes, excluded from inputs,
  filters, sorts; typed params surface on read calls.
- LSP: `@computed` added to attribute completion if a list exists.
