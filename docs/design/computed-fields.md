# Computed fields (`@computed`) — resolver-backed response-time fields

Status: implemented (v1). Source of truth for the `@computed` feature.

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
- **The server's embedded self-client decodes into server-side structs and
  silently drops computed fields in v1** (tracked follow-up). `include_server_schema!`
  generates an internal `cratestack_schema::client::Client` for self/peer calls
  (`crate::client::generate_client_module`, shared with standalone
  `include_client_schema!`), but its per-model methods return
  `super::models::<Model>` — the server-side struct type
  (`generate_model_struct_only`), which excludes computed fields by design. Only
  a *standalone* `include_client_schema!` call (its own `models`/`types` modules,
  built by `generate_client_model_struct`/`generate_client_type_struct`) gets the
  client-side struct shape that actually carries computed field values.
- **RPC transport model reads have no `computedParams` slot** — `?computedParams=`
  is a REST query-string parameter only; RPC unary/batch dispatch for
  `model.<Model>.get`/`model.<Model>.list` never reads or threads one through
  (procedure output composition, unlike model reads, works identically under
  both transports).
- **Create/update/delete commit the DB write before resolvers run.** The
  handler calls `.create()`/`.update()`/`.delete()` (each a real,
  already-committed write) and only afterward runs response composition
  (which invokes resolvers). A resolver error therefore always describes an
  error *response* for a write that already happened — there is no
  transactional rollback tying resolver success to the write.
- **`computedParams` value decoding is not pre-DB.** Only the *keys* of a
  `?computedParams=` object are validated before any database access (does
  the key name a parameterized computed field of this model, is it excluded
  by `?fields=`, is the payload even a JSON object at all). Decoding a key's
  *value* into its field's declared params type
  (`serde_json::from_value::<ParamsType>`) happens later, at
  response-serialization time, after the row (or rows) has already been
  fetched — see `cratestack-macros/src/axum/model/serializers/computed_fields.rs`.
- **Unknown keys *inside* a params object are silently ignored** — standard
  serde struct deserialization, not `#[serde(deny_unknown_fields)]`. Only the
  top-level `computedParams` object's keys (the computed field names) are
  validated; an extra, unrecognized key inside one field's params payload
  (e.g. `{"proxyUrl": {"width": 800, "typo": true}}`) is dropped, not
  rejected.

## Downstream

- `cratestack-migrate`: computed fields excluded from DDL/diff.
- Wiremock generator: computed fields fabricated like ordinary response fields.
- Dart/TS clients: computed fields in response classes, excluded from inputs,
  filters, sorts. Both expose a `computedParams` surface on `get`/`list`, but
  it is an **untyped v1 escape hatch** in both — Dart's is
  `Map<String, Object?>?`, TypeScript's is `Record<string, unknown>` — not a
  generated per-model params type; a typed wrapper is tracked follow-up work.
  Dart additionally gates the parameter per model (offered only when the
  model has at least one *parameterized* `@computed(params: <Type>?)` field —
  a bare-`@computed`-only model never gets it, since the server would 422 any
  `computedParams` key for a field with no params type); TypeScript's
  `computedParams` lives on one shared query type used by every model's
  `get`/`list`, so it has no equivalent per-model gate.
- **The generated Rust client (`include_client_schema!`) has no
  `computedParams` surface at all in v1** (tracked follow-up) — the
  `?computedParams=` query parameter has no constructor, builder method, or
  argument anywhere in `cratestack-macros/src/client/`. It still *decodes*
  computed field values correctly on responses (via the client-side struct
  shapes described above), it just cannot request non-default resolver
  parameters.
- LSP: `@computed` added to attribute completion if a list exists.
