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

Both transports carry the same logical payload — a JSON object keyed by computed
field name, each value deserializing into the generated params `type` struct via
serde — just in different envelopes.

**REST.** Model GET/list requests accept one query parameter:

```
?computedParams=%7B%22proxyUrl%22%3A%7B%22width%22%3A800%7D%7D
   (URL-encoded {"proxyUrl": {"width": 800}})
```

**RPC.** `model.<X>.list` decodes `RpcListInput`, which carries a
`computedParams` field (raw JSON-object text — see below for why not
`serde_json::Value`); `model.<X>.get` decodes a dedicated `RpcGetInput { id,
computedParams }` rather than reusing `RpcPkInput` (which `delete` also
decodes, and would otherwise gain a silently-ignored field). Server-side, the
RPC dispatcher synthesizes the equivalent `?computedParams=` query string from
the decoded field and hands it to the exact same fetch/list query parser REST
uses (`parse_model_fetch_query`/`parse_model_list_query`,
`cratestack-macros/src/axum/shared_support.rs`) — one validation
implementation, no drift between transports. On `/rpc/batch`, each frame
carries its own `computedParams` inside that frame's `input`, resolved
independently per frame; in-frame params are signed by construction, since the
canonical signed body under `transport rpc` is the raw frame bytes themselves
(`docs/design/rpc-transport.md` §5).

Why `computedParams` is a `String` (raw JSON text) on the RPC input types
rather than `serde_json::Value`: `RpcUpdateInput`'s own doc comment already
documents that round-tripping an `Option`-bearing value through
`serde_json::Value` corrupts CBOR `Option::None` (`minicbor-serde` encodes it
as `0xf6` simple-null, `serde_json::Value` encodes it as the CBOR empty-array
marker) — generated params types are bags of optionals, so they'd hit this
head-on. `/rpc/batch` additionally re-encodes each frame's opaque `input`
through `serde_json::Value` before re-dispatching it
(`cratestack-axum::rpc::batch`); a `String` field survives that round trip
verbatim, a nested object wouldn't.

Both transports:

- Malformed JSON, unknown field keys, keys naming non-computed or param-less
  fields, or a params payload for a field excluded by `?fields=` (REST only —
  RPC `get` has no `fields`/`include` slot, see below) →
  `CratestackError::Validation`, same HTTP status either way.
- Absent `computedParams` (or absent key) → resolver gets `None`.
- Applies to the request's *root* model only in v1; relation-included records and
  all non-read paths resolve with `None`.
- Generated REST clients (Rust/Dart/TS) expose typed optional params on
  get/list calls and serialize them into the query parameter; the generated
  RPC client surface for `computedParams` is tracked separately (see
  "Downstream" below).

**Deliberate asymmetry: RPC `get` has no `fields`/`include`.** Unlike
`RpcListInput` (which mirrors the REST list query 1:1, including `fields`),
`RpcGetInput` carries only `id` and `computedParams`. RPC `get` always decodes
its response into the full generated model type in every client, which has no
representation for a partial (fields-selected) payload — so the REST
"excluded by `?fields=`" rejection branch is reachable over RPC list but
unreachable over RPC get. This is a scope limit, not a gap.

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
- **The generated Rust client** (both `include_client_schema!` and the
  server's own embedded self-client, since both go through the single
  `crate::client::generate_client_module` call site) has a **typed**
  `computedParams` surface, unlike Dart/TS's untyped escape hatches above —
  `cratestack-macros/src/client/computed_params.rs` emits one
  `<Model>ComputedParams` struct per model with at least one *parameterized*
  `@computed(params: <Type>?)` field (same per-model gate Dart uses; a
  bare-`@computed`-only model gets neither the struct nor an extra
  parameter), with one `Option<super::types::<Params>>` field per resolver
  and a `to_query_value()` helper that serializes to the same JSON-object
  text both transports expect (`None` when every field is unset, matching
  the server's "absent key -> resolver gets `None`" default). `get`/`list`
  on a gated model's REST client take an extra `computed_params:
  Option<&<Model>ComputedParams>` parameter; RPC's `get` switches from
  `RpcPkInput` to `RpcGetInput { id, computed_params }` and `list` clones
  its `RpcListInput` and overwrites `computed_params` with the typed
  struct's encoded value. An ungated model's `get`/`list` tokens are
  unchanged from before this surface existed, including RPC `get`'s
  `RpcPkInput` shape.
- LSP: `@computed` added to attribute completion if a list exists.
