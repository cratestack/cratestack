# ADR 0019: `Int` and `BigInt` as built-in integer types

## Status

Accepted

Accepted 2026-09-30 by the maintainer; D2 decided explicitly, D3 to D5 and the release decided on
the ADR's recommendations. Every question this ADR left open is now a recorded decision:

| Decision | Recorded outcome | Basis |
|---|---|---|
| D2, CBOR | `BigInt` is a CBOR text string (major type 3) holding the canonical decimal form, with JSON's grammar and rejection rules. Native CBOR integers and RFC 8949 bignum tags 2 and 3 are rejected. | Explicit: CBOR is the primary codec for these projects, so a decision for it is mandatory; `BigInt` as a string is fine |
| D3, Rust type | `cratestack::BigInt`, a newtype over `i64`: `get`/`new`, `From<i64>`, `From<BigInt> for i64`, checked arithmetic only, no `Deref`. The name stays. Amended in PR B: the driver impls live in `cratestack-core` behind optional features (Amendment below). | The ADR's recommendation, accepted |
| D4, upgrade | `cratestack upgrade int-to-bigint` rewrites every `Int` to `BigInt` by parser span; no DDL. Narrowing to `Int` is a per-field edit, `Lossy` in `migrate diff`, behind `--allow-destructive`. | The ADR's recommendation, accepted |
| D5, field attributes | Closed list per declaration kind; supersedes the narrow route taken on cratestack#679 for field attributes. | The ADR's recommendation, accepted |
| Release | PR A (D5), PR B (`BigInt` end to end) and PR C (the `Int` cutover) all ship in **0.16.0**, one breaking release, no compatibility path. | The ADR's recommendation, accepted |

> **Placement note.** `docs/adr/README.md` sends decisions about the user-visible surface
> (`.cstack` grammar, transport semantics, migration behaviour) to `cratestack-docs/internals/`.
> This one changes all three. It is filed here, as ADR 0018 was, because it was asked for in this
> repository and because its argument is made of crate-internal paths from start to finish. Move
> it if the maintainer reads the split the other way. 0019 is the next free number in both
> repositories: `cratestack-docs/internals/` holds 0001 to 0006, 0007 to 0010 are reserved, and
> this directory ends at 0018.

## Date

2026-09-30 (proposed, this PR); 2026-09-30 (accepted by the maintainer, this PR). Decision
requested by the maintainer on 2026-09-30: "Fix CrateStack. Propose Int and BigInt as built-in
types."

Context doc: [`docs/design/int-and-bigint.md`](../design/int-and-bigint.md), which carries the
per-target changes with `path:line` citations, the sequence and state diagrams, the implementation
plan and the evidence. Every `path:line` here and there was READ on `origin/main` at `67100917`
(v0.15.1; `main` has since gained #1127 at `4c7e25cb`, which shifts a few of those lines); every
behaviour marked RAN was executed for this ADR and is reproduced in the context
doc's §9 under the key given (E1 to E12).

## Context

A schema `Int` generates a Rust `i64` with no serde attribute
(`crates/cratestack-macros/src/shared/types.rs:34`, `shared/wire_types.rs:49`,
`procedure/type_tokens.rs:26`, `:96`), a `BIGINT` column
(`crates/cratestack-migrate/src/emit/postgres/columns.rs:270`), and a JSON number carrying every
digit (`crates/cratestack-codec-json/src/lib.rs:11`; RAN, E5). JSON Schema promises the whole
`i64` range (`crates/cratestack-macros/src/json_schema/scalar.rs:66`, `:111-113`).

The clients cannot hold that range:

- TypeScript types `Int` as `number` (`crates/cratestack-client-typescript/src/types.rs:48`) and
  parses bodies with `JSON.parse` (`templates/src/rest-runtime.ts.j2:190`,
  `templates/src/rpc-runtime.ts.j2:73`). `9007199254740993` arrives as `9007199254740992`, typed
  `number`, with no error (RAN, E6).
- Dart types `Int` as `int` (`crates/cratestack-client-dart/src/dart_types.rs:38`). That is exact
  on the VM and on dart2wasm and rounds on dart2js, Flutter web's JavaScript build (RAN, E7).
- Over CBOR, `@cratestack/cbor-node` returns a `number` up to 2^53 - 1 and a `bigint` above it
  (`crates/cratestack-cbor-napi/src/js_value/napi_conversions.rs:99-106`) and
  `@cratestack/cbor-web` refuses to decode the response at all (serde-wasm-bindgen 0.6.5
  `src/ser.rs:329-346`, called from `crates/cratestack-cbor-wasm/src/wasm.rs:86-88`). Both READ.

So every `i64` above 2^53 can be corrupted in every JavaScript consumer, silently. The reported
victim is skyport-billing, which stores money as `i64` e8 units: past 90,071,992.54740991 XAF an
amount with a sub-franc part no longer survives the portal (RAN, E6).

There is no way to fix it for one field. A schema author who reaches for `amountE8 Int @string`
or `@wire(string)` gets `schema OK` and an unchanged `amountE8: number`; the attribute moves
nothing but the schema digest (RAN, E1, E4). Field attributes are the last position where an
unknown name is inert: model and view `@@` attributes, procedure attributes and query attributes
became closed lists in 0.14.1 (GHSA-69g4-xvcm-vm2j; `CHANGELOG.md:229-300`), while field
attributes kept cratestack#679's option (b), which refuses only a near-miss of a known name
(`crates/cratestack-parser/src/validate/misspelled_attributes.rs:27-32`, `:213-240`). #679's
close-out left the closed set for "its own ticket with the migration story it implies"
([comment](https://github.com/cratestack/cratestack/issues/679#issuecomment-5458906334)).

The constraints this decision works under: the public crates version together pre-1.0 and take
breaking changes with a minor bump and a changelog entry (ADR 0017, ADR 0018); the maintainer's
standing rule is a hard cutover, with no parallel path and no default-off flag; REST and RPC and
every generated client change in the same PR (`CLAUDE.md`, "Transport parity"); and
`cratestack-docs` and `cratestack-skills` must be declared on every user-facing change (PR
template section 9).

## Decision

**D1. `Int` is a 32-bit signed integer and `BigInt` is a 64-bit signed integer, as in Prisma.**

| | `Int` | `BigInt` |
|---|---|---|
| Range | `i32` | `i64` |
| Rust | `i32` | `cratestack::BigInt`, a `Copy` newtype over `i64` (D3) |
| Postgres | `INTEGER` | `BIGINT` |
| SQLite | integer storage, `BLOB` affinity as today | same |
| JSON | number | canonical decimal string (D2) |
| CBOR | major type 0/1 integer | the same decimal string, as a text string (D2) |
| TypeScript | `number` | `bigint` |
| Dart | `int` | `BigInt` |
| JSON Schema | `integer`, `i32` bounds | `string`, decimal pattern |

`Int` is exact in every consumer by construction, because 2^31 is far inside every number type
the clients have. `BigInt` is exact because it never travels as a number. Both are built-in
scalars: `BigInt` joins `BUILTIN_TYPES`
(`crates/cratestack-parser/src/validate/type_names.rs:7-24`), is valid wherever `Int` is (fields,
procedure arguments and returns, `query` parameters, `@version`, `@range`, `@default`, policy
literals, find-many filters, MCP resource keys), and has its own filter type on every client.
The full per-target table is the context doc's §3.

**D2. A `BigInt` travels as a canonical decimal string on every codec, JSON and CBOR alike, and
only that form is accepted.** Decided by the maintainer on 2026-09-30, in substance: CBOR is the
primary codec for these projects, so a decision for it is mandatory, and `BigInt` as a string is
fine. CBOR's encoding is therefore part of this decision in full, not a note on the JSON one.

- **The form.** `0`, or an optional `-`, a non-zero digit and up to 18 more digits, inside `i64`.
  Emitted by `Display`, so the server only writes the canonical form.
- **JSON.** A JSON string holding that form.
- **CBOR.** A text string (major type 3, [RFC 8949 §3.1](https://www.rfc-editor.org/rfc/rfc8949#section-3.1))
  holding that same form, with the same grammar and the same rejection rules as JSON: no leading
  `+`, no leading zeros, no `-0`, no surrounding whitespace, range-checked to `i64`, and no number
  form accepted. `i64::MAX` is the initial byte `0x73` (major type 3, length 19) and its 19 ASCII
  digits, 20 bytes, where today's `Int` takes 9 (`1b7fffffffffffffff`, RAN, E5). `Decimal` already
  has this shape: `1.50` encodes as `64312e3530` (RAN, E5).
- **Refused on both codecs, with a message naming the field.** A JSON number; a CBOR integer
  (major type 0 or 1); a CBOR bignum (tag 2 or 3); `+5`, `007`, `-0`, `" 1"`; anything outside
  `i64`. The prototype's `Deserialize` implements only `visit_str`, so every other item is refused
  by construction on either codec; the JSON refusals RAN (E5), and PR B's tests pin the CBOR ones
  (design doc §7). A number is refused because a value above 2^53 may already have been rounded by
  the JavaScript that produced it, and the server cannot tell. That is also the rule `Decimal`
  follows today (`rust_decimal` with `serde-str`, root `Cargo.toml:405`, refuses a JSON number;
  RAN, E5).

**Why a text string on CBOR, and not a native integer.** A native integer is smaller and is what
CBOR is for. Three facts in the tree, all READ, rule it out:

1. *Batch frames.* `POST /rpc/batch` carries every frame's input and output as `serde_json::Value`
   (`crates/cratestack-core/src/rpc.rs:107`, `:118`), so a batched value passes through JSON's
   data model on its way to CBOR. An encoding that wrote a string for JSON and an integer for CBOR
   would branch on `is_human_readable()`; a batch frame takes the JSON branch and lands on CBOR as
   the JSON string, so one field would have two CBOR forms depending on the route. That is the
   `Uuid` defect `ProjectedValue` was built to avoid on the unary path
   (`crates/cratestack-axum/src/projection.rs:1-17`). A string on both codecs has no branch to take
   (prototype RAN through `serde_json::Value` then CBOR, E5).
2. *The Dart codec.* The `cratestack_cbor` codec, the default
   (`crates/cratestack-client-dart/src/config.rs:17`), crosses a JSON-text boundary on both of its
   platforms (`dart-packages/cratestack_cbor/lib/src/cbor_codec.dart:17-31`), and on the web that
   text goes through JS `JSON.parse` and `JSON.stringify`
   (`lib/src/web/web_cbor_codec.dart:75`, `:95`), which round a number above 2^53 and throw on a
   `bigint`. A string survives both.
3. *The two JS bridges disagree about large integers.* `@cratestack/cbor-node` returns a `number`
   up to 2^53 - 1 and a `bigint` above it
   (`crates/cratestack-cbor-napi/src/js_value/napi_conversions.rs:99-106`), so one field arrives as
   two JS types depending on its value; `@cratestack/cbor-web` refuses to decode the response at
   all (serde-wasm-bindgen 0.6.5 `src/ser.rs:329-346`, called from
   `crates/cratestack-cbor-wasm/src/wasm.rs:86-88`). A string needs neither bridge changed.

**Why a text string, and not an RFC 8949 bignum (tag 2 or 3).**
[RFC 8949 §3.4.3](https://www.rfc-editor.org/rfc/rfc8949#section-3.4.3) defines bignums for
integers that do not fit major types 0 and 1, and its preferred serialization never uses one for a
value that does; every `i64` fits. A tagged byte string would be a second, non-preferred spelling
of a number that already has a native form, and the RFC's security considerations
([§10](https://www.rfc-editor.org/rfc/rfc8949#section-10)) warn that a decoder in the basic data
model gives the two spellings different semantics. Accepting both is the two-forms path this
decision excludes; emitting only the tag makes every value a non-preferred encoding. It also has
nowhere to go on the paths above: `serde_json::Value` frames and the Dart JSON-text boundary have
no tag, and both JS bridges decode into `cratestack_core::Value`, which has `Int(i64)`, `String`
and `Bytes` and no tag or bignum variant (`crates/cratestack-cbor-wasm/src/wasm.rs:16`,
`crates/cratestack-cbor-napi/src/js_value/napi_conversions.rs:50`,
`crates/cratestack-core/src/value.rs:20-29`). RFC text READ on 2026-09-30.

**Consequence: one wire form on every codec.** A value decodes identically whichever codec a
client negotiates: the same field is the same string in a JSON body, a CBOR body, a batch frame and
a `?fields=` projection, and no client needs a codec-specific branch for it. The cost is size: 20
bytes for `i64::MAX` against 9. Native CBOR integers or bignums come back only through a new ADR
that reworks those three paths first and supersedes this decision, never as a flag or as a second
accepted form.

**D3. The Rust type is a newtype, `cratestack::BigInt`, in `cratestack-core`.** Accepted on the
ADR's recommendation. Its `Serialize` writes the decimal string and its `Deserialize` accepts only
D2's form. A raw `i64` with a `#[serde(with = ...)]` attribute was the alternative, and it fails
silently on every path that does not see the struct field's attributes: `?fields=` projections
serialize each field as a type-erased leaf
(`crates/cratestack-macros/src/axum/model/serializers/projection_fields.rs:24`,
`crates/cratestack-axum/src/projection.rs:72`), a bare procedure return is the value itself,
`FieldFilterInput<T>` is generic, and audit snapshots are `serde_json::to_value(&record)`
(`crates/cratestack-sqlx/src/query/write/create.rs:92`). With a newtype each of those is right
by construction, and a place that still expects `i64` is a compile error. The database boundary
converts in generated code (`try_get::<i64>` then `BigInt::new`; `SqlValue::BigInt(i64)`), so the
newtype needs no sqlx or rusqlite impls and `cratestack-core` keeps zero workspace dependencies.
**Superseded 2026-10-09, see the Amendment (PR B) below:** the newtype does need the driver impls
for a `BigInt` primary or foreign key, and they live in `cratestack-core` behind two optional
features. `cratestack-core` still has no workspace dependency.

The decided surface: `new` and `get`, `From<i64>` and `From<BigInt> for i64`, `Display`, `FromStr`
and the comparison traits, and arithmetic as checked methods only (`checked_add`, `checked_sub`
and `checked_mul`, each returning `Option<BigInt>`; PR B fixes the final list). There are no
`Add`, `Sub`, `Mul` or `Neg` operator impls, because an operator on a 64-bit money value must
panic, wrap or invent an error path. There is no `Deref`: with one, `i64` methods and a `&BigInt`
passed where a `&i64` is expected would keep compiling at a call site nobody updated, which is the
silent path the newtype exists to close. The rationale is one sentence: a missed call site becomes
a compile error instead of silent precision loss. The name stays. It is namespaced
(`cratestack::BigInt`, so it does not collide with `num_bigint::BigInt` unless a file imports
both) and it matches the schema type; the rustdoc says it is 64-bit, not arbitrary precision.

**D4. The change ships as one breaking release, 0.16.0, with a codemod and no compatibility
path.** Accepted on the ADR's recommendation: `cratestack upgrade int-to-bigint` rewrites every
`Int` to `BigInt` by parser span, so storage and wire width are preserved and no DDL is emitted.
Narrowing a field to `Int` is a deliberate per-field edit that `migrate diff` treats as lossy,
behind `--allow-destructive`. Owners narrow at their own pace; the tool never schedules a table
rewrite for them.

- **`cratestack upgrade int-to-bigint --schema <file>... [--migrations <dir>] [--check]`**
  rewrites every type reference named `Int` to `BigInt` by its parser span (so comments, strings
  and SQL bodies are untouched), re-parses to prove only those names changed, and rewrites each
  `schema.snapshot.json` under `--migrations` from format 2 to format 3 with `Scalar("Int")`
  renamed `Scalar("BigInt")`. It is idempotent, and `--check` is the CI form. After it, storage is
  identical and `migrate diff` emits no DDL; every former `Int` now travels as a string and
  generates `bigint`/`BigInt`, which is the fix, applied to every field the tool cannot prove is
  small. It ships in 0.16.0 and is deleted in 0.17.0.
- **`cratestack-migrate` snapshot format 3.** `ColumnType::Scalar` keeps the `.cstack` name
  (`crates/cratestack-migrate/src/ir/columns.rs:33-39`), so the name's new meaning is a format
  change. A format-2 snapshot is refused (`snapshot.rs:102-108`) with a message naming the
  command, the same no-shim precedent #205 set for format 1.
- **Narrowing is a separate, deliberate edit.** Moving a field from `BigInt` back to `Int` makes
  `migrate diff` emit `ALTER COLUMN ... TYPE INTEGER` (`emit/postgres/columns.rs:46-57`), classed
  `Lossy` (`ir.rs:93`) and so requiring `--allow-destructive`
  (`crates/cratestack-cli/src/migrate/diff_cmd.rs:56`). Postgres rewrites the table and aborts the
  migration with `22003` if any row does not fit. Introspection maps `int4` to `Int` and `int8` to
  `BigInt` (today `int4` is unmapped, `introspect/postgres/types.rs:32`, `:53-60`).
- **Digest domains move to `v2`.** `SCHEMA_IDENTITY_DOMAIN`, `OP_CONTRACT_DOMAIN` and
  `CLIENT_CONTRACT_DOMAIN` (`crates/cratestack-core/src/schema_identity.rs:43`,
  `client_contract.rs:53-55`). `client_contract.rs:17-19` says the domain moves when the derivation
  rules change; a name that changes meaning is such a change, and without the move a 0.15 client
  and a 0.16 server built from the same text would agree on every digest while disagreeing on the
  wire. Every digest changes once, as `SCHEMA_SHA256` did in 0.15.0 (#1065). Since #1127 merged
  (binding version 2, `4c7e25cb`, after this worktree's base) a signed request binds its op's
  contract digest, so every signed client answers the unsigned `426 contract_unsupported` on every
  op until it is regenerated: the right outcome, because every op's wire meaning may have changed.
  The codemod's renames additionally move the digest of each op that reaches a rewritten field, as
  #1123 designed.
- **What a user does.** Run the command, commit the schema and snapshot changes, regenerate every
  client and rebuild every server together, then narrow fields to `Int` one at a time when they
  choose to pay the table rewrite. A user who skips the command is not served wrong data: a
  format-2 snapshot is refused; on Postgres without a snapshot every read fails, because sqlx 0.9
  decodes `i32` only from `INT4` (sqlx-postgres 0.9.0 `src/types/int.rs:108-112`,
  `src/type_info.rs:1065-1070`, READ); without a database or on SQLite, values outside `i32` are
  refused. The state diagram in the context doc's §6 draws each path and the one it makes
  unreachable.
- **CHANGELOG.** Breaking entries under `## Unreleased`, in the voice of the 0.14.1
  GHSA-69g4-xvcm-vm2j entry, the command first.

**D5. Field attributes become a closed list, per declaration kind.** Accepted on the ADR's
recommendation. The companion decision, and #679's option (a): it supersedes the narrow route
(option (b), shipped in #810) that #679 took for field attributes. Each of `model`, `view`,
`mixin`, `type` and `auth` gets a table of the field attributes it accepts, checked by the same
`check_shape` (`crates/cratestack-parser/src/validate/attribute_shape.rs:38`) that already closes
block, procedure and query attributes, so an unknown name, another case or a stray argument list
is an error, with a suggestion when a known name is close. The union the readers use today is 19
names (context doc §5), and no field attribute outside it appears in any of the 265 committed
schemas that parse or in the five downstream schemas (RAN, E9), so the migration cost is nil for
every schema counted. That census is the migration story #679 asked for, and the answer to the
objection recorded at `misspelled_attributes.rs:17-25`. This is what turns `@string` from a
silent no-op into the error that sends its author to `BigInt`.

The concern that decided it is a mistyped protection, `@readonly` above all, leaving a field
writable. Exactly which mistakes do that, on the 0.15.1 build: a near-miss of `@readonly` is
refused (seven variants, RAN, E12), because #679's option (b) catches a typo of a known name. What
option (b) cannot catch is an attribute that is not close to any known name. `@immutable`,
`@string` and `@wire(string)` each report `schema OK` (RAN, E12) and nothing reads them (E4 shows
it for `@string`), so the protection the author believes they wrote is absent and the field stays
writable. A closed list turns each of those into an error. D5 does not depend on D1 to D4
technically, and it ships in the same release as PR A (Release, below).

**Release: 0.16.0, all three pull requests together.** PR A (closed field attributes, D5), PR B
(`BigInt` end to end, D2 and D3) and PR C (the `Int` cutover, D4) ship in **0.16.0**, as one
breaking release, with no compatibility path and no release cut between them. The workspace is at
0.15.1 and #1127 already sits under `## Unreleased` as a breaking change, so 0.16.0 is the next
minor, which is where a pre-1.0 breaking change goes (ADR 0017, ADR 0018). A user takes one
upgrade: the closed attribute list, `BigInt`, the 32-bit `Int` and `cratestack upgrade
int-to-bigint` arrive together, and the command is deleted in 0.17.0.

**How this honours the request.** `Int` and `BigInt` are both built-in, first-class scalars with
their own type on every target, their own column type, their own filter type and their own wire
form; neither is an attribute, an alias or a flag on the other, and the old meaning of `Int` does
not survive the release anywhere.

## Amendment (PR B, 2026-10-09)

PR A (the closed field attributes, D5) and PR B (`BigInt` end to end, D2 and D3) land on `main`
together, ahead of PR C (the `Int` cutover, D4). Building PR B found five places where this ADR or
its context doc was wrong or silent. Where this section and an earlier one disagree, this section
governs; the earlier text is left as it was written. Labels follow the repository convention: RAN
means executed on 2026-10-09 (macOS arm64) with the output observed, READ means source was read.

| Item | Outcome | Replaces |
|---|---|---|
| D3, driver impls | `cratestack-core` has two optional features, `sqlx-postgres` and `rusqlite`, that carry the `Type`/`Encode`/`Decode` and `ToSql`/`FromSql` impls for `BigInt`. The Postgres one is required; nothing but a unit test needs the SQLite one. | D3: "the newtype needs no sqlx or rusqlite impls" |
| Policy claims | A string auth claim in a numeric policy comparison is undecidable and denies. Coercing a canonical string is a follow-up. | The context doc's §3.2 sentence on claims |
| D2, error text | A refused value is named by field path in `CratestackError::Codec`'s `detail()`; the public message stays generic. | D2: "with a message naming the field" |
| Dart | Decode goes through `cratestackDecodeBigInt` with no client-side `i64` range check; RPC `get`, `update` and `delete` send a `BigInt` key as a string. | The context doc's §3.6 |
| RTK floor | `@reduxjs/toolkit` rises from `^2.0.0` to `^2.2.7`. | The context doc's §3.5: "the first release carrying that commit" |
| Pending | B13 (a live-server TypeScript round trip) and PR C. | |

**D3: the driver impls live in `cratestack-core`, behind optional features.** D3 said the database
boundary converts in generated code (`try_get::<i64>` then `BigInt::new`), so the newtype needs no
sqlx or rusqlite impls. That is wrong on Postgres and only half right on SQLite.

- *Postgres.* The generated row decode is `row.try_get(name)?` for every plain scalar
  (`crates/cratestack-macros/src/model/row_pg.rs:87`, `:160`), so a `BigInt` field is read as a
  `BigInt` and needs `Type<Postgres>` and `Decode<Postgres>`. A `BigInt @id` or foreign key also
  meets the delegates' bounds, `PK: Send + sqlx::Type<Postgres> + for<'q> Encode<'q, Postgres>`
  (READ: `crates/cratestack-sqlx/src/delegate/scoped_writes.rs:87`,
  `crates/cratestack-sqlx/src/audit.rs:148`). The orphan rule lets only the crate that owns `BigInt`
  or the crate that owns the trait write the impls, and the trait crates are not ours. So the impls
  are in `cratestack-core`.
- *SQLite.* The embedded delegates bound a key with `IntoSqlValue`
  (READ: `crates/cratestack-rusqlite/src/delegate/delete.rs:17`), which `cratestack-sql` implements
  for `BigInt` (`crates/cratestack-sql/src/values/into_sql.rs:24`), and the generated row decode
  reads an `i64` and applies `BigInt::new` (`crates/cratestack-macros/src/model/row_sqlite.rs:103-108`),
  so no `ToSql` or `FromSql` impl on `BigInt` is on that path. The `rusqlite` feature adds them
  anyway, for hand-written rusqlite code. RAN, on a scratch copy of this tree with
  `features = ["rusqlite"]` removed from `cratestack-rusqlite`'s dependency on `cratestack-core`:
  `cargo check -p cratestack-rusqlite --lib` and `cargo check -p cratestack-sqlite --all-targets` both
  finish, and the only failure is `cratestack-rusqlite`'s own unit tests
  (`src/value/tests_bigint.rs:92`, `:127`, `:152`), which call `row.get::<_, BigInt>`. Other
  rusqlite users in the workspace (studio, the examples) were not built. So this feature is
  enabled by `cratestack-rusqlite` on D-PK's reasoning and is exercised only by tests; whether it
  stays is left to the maintainer (`ROADMAP.md`). The Postgres feature is not in doubt.

The result:

- `cratestack-core` gains `sqlx-postgres = ["dep:sqlx-core", "dep:sqlx-postgres"]` and
  `rusqlite = ["dep:rusqlite"]` (`crates/cratestack-core/Cargo.toml:55-56`, optional dependencies at
  `:74-76`). They add `Type`, `Encode`, `Decode` and `PgHasArrayType` (`INT8`, `INT8[]`) and `ToSql`
  and `FromSql` (an SQLite integer), each delegating to `i64`. The pair `sqlx-core` and
  `sqlx-postgres` is what `cratestack-sqlx` already depends on, not the `sqlx` umbrella, for the
  same `links = "sqlite3"` reason the facades avoid it.
- Only the runtime that needs a feature enables it: `cratestack-sqlx` turns on `sqlx-postgres`
  (`crates/cratestack-sqlx/Cargo.toml:84`) and `cratestack-rusqlite` turns on `rusqlite`
  (`crates/cratestack-rusqlite/Cargo.toml:52`). Neither is a default and none is a flag that gates
  behaviour; they are optional dependencies, not a dormant path.
- `cratestack-core` still has no workspace dependency, so it is visible to all four facades, which
  was the point of D3. The facade-disjointness invariant (ADR 0013) holds. RAN, from the repository
  root and in the standalone example workspaces, with the same commands CI's `facade-disjointness`
  job uses:

  ```text
  examples/no-database-verification-api   cargo tree --locked | grep -ci sqlx        -> 0
  examples/client-only-verification       cargo tree --locked | grep -ci sqlx        -> 0
  examples/client-only-verification       cargo tree --locked | grep -ci axum        -> 0
  examples/no-database-verification       cargo tree --locked | grep -ci sqlx        -> 0
  cargo tree -p cratestack-api    | grep -ci sqlx                                    -> 0
  cargo tree -p cratestack-client | grep -ci sqlx                                    -> 0
  cargo tree -p cratestack-pg     | grep -ci libsqlite3                              -> 0
  cargo tree -p cratestack-sqlite | grep -ci sqlx                                    -> 0
  cargo tree -p cratestack-core -e normal | grep -ciE "sqlx|rusqlite|libsqlite3"    -> 0
  ```

  `cargo metadata --locked` succeeds in all three examples, so their lockfiles carry the new
  optional edges. (`cratestack-sqlite` does show `libsqlite3-sys`, as it must.)

**Policy claims: a string claim in a numeric comparison is undecidable, and denies.** The context
doc's §3.2 said a `BigInt` claim "is accepted as a JSON integer or a canonical string". That is true
of `@default(auth().x)` only, where the value is parsed once with `BigInt::from_str` and anything
else is refused (READ: `crates/cratestack-sqlx/src/query/support/create.rs`, the `CreateDefaultType::BigInt`
arm). In a policy comparison it is not, for three reasons, all READ in the code:

1. A predicate carries no column type, so the evaluator cannot tell a `BigInt` string from a
   `String` one.
2. The pushed-down SQL cannot either: a string claim renders as `$N::text`, and Postgres has no
   `bigint = text` operator, so the database refuses the comparison
   (`crates/cratestack-sqlx/src/query/support/values.rs`, `claim_type_suffix`). The cast also gives
   each claim type its own statement text, which closes a fail-open that sqlx's statement cache by
   SQL text allowed between an integer claim and a later string claim.
3. Reading the string as a number in the evaluator would make one claim pass in a procedure policy
   and fail in a model policy.

So a `BigInt` claim must be a JSON integer in a policy comparison. The same decision is made twice,
once for the create path and `auth().x` literals (`comparison.rs` in `cratestack-sqlx`: `claim_vs_literal`,
`column_vs_claim`) and once for procedure policies (`compare.rs` in `cratestack-policy`), and each
file names the other. Both are three-valued: equal, different, or undecidable. `==` and `in` pass
only on a decided equality, `!=` and `not in` only on a decided difference, and an undecidable pair
satisfies neither, so a `@deny` on it fires. This is what SQL does with a `NULL`. The create-path
evaluator composes the outcomes with Kleene `and` and `or`: an `@allow` grants only on a decided
true and a `@deny` fires on anything but a decided false. On read, update and delete the
pushed-down `@deny` on `auth().x <op> <literal>` renders `NULL` rather than `FALSE` for an
undecidable claim, so it fires there too.

Three limits are accepted and recorded. In procedure policies only an integer against a string is
undecidable; a `Bool`, `Float` or `Null` operand still passes `!=`. The model pairs that predate
`BigInt` keep derived equality, so a `NULL` `Int?` column against a claim still satisfies a
create-path `!=` where SQL denies. And an absent or unbindable claim (null, float, bytes, list, map)
makes a column comparison false on create, read, update and delete, so a
`@deny(owner != auth().accountId)` stays silent for a caller whose claim is null (a `NULL` `Bool`
column under `FieldIsTrue` is the same family). All three are follow-ups, and the second must be
settled before PR C, because `NullBigInt` is stricter and the codemod would change outcomes. Coercing a canonical-string
claim in policies (the design the context doc first described) is a follow-up too; it needs the
predicate to carry the column type.

**D2: the field path is in `detail()`.** D2 says a refused value gets "a message naming the field".
`JsonCodec` and `CborCodec` now decode through `serde_path_to_error`, so the path is in the error's
`detail()` (`failed to decode JSON body: amountE8: ...`), while the public message of
`CratestackError::Codec` is unchanged and generic, because it reaches clients. The wire form, the
grammar and the refusals of D2 are as decided.

**Dart: two deviations from the context doc's §3.6.**

- *Decode.* The doc said `BigInt.parse({expr} as String)`. `BigInt.parse` is not strict. RAN on the
  Dart VM: it returns `5` for `+5`, `7` for `007`, `0` for `-0`, `31` for `0x1F`, `1` for `" 1"` and
  accepts `9223372036854775808`, every one a spelling D2 refuses. The generated runtime has
  `cratestackDecodeBigInt(value, 'Owner.field')`
  (`crates/cratestack-client-dart/templates/rest-runtime.dart.j2:66`), which checks the type and the
  canonical grammar and throws a `FormatException` naming the field, then calls `BigInt.parse`, which
  is exact on the VM, dart2js and dart2wasm. It does not check the `i64` range: the server refuses a
  value outside `i64`, and a Dart `BigInt` is not bounded.
- *RPC keys.* `get`, `update` and `delete` took `{'id': id}`, which for a `BigInt` key is a Dart
  `BigInt`: `jsonEncode` throws on it and `package:cbor` writes an integer or a bignum, not the text
  string the server requires. They send `{'id': id.toString()}`
  (`crates/cratestack-client-dart/templates/rpc-apis.dart.j2:101-143`, `primary_key_wire_expr` built
  from `encode_value_expr`), in the default and the Riverpod presets.

**The RTK floor is 2.2.7, not the first release with the bigint commit.** §3.5 raised the floor to
the first release carrying reduxjs/redux-toolkit@ae838b4c, which is 2.2.4. Two facts set the floor
and it is the later one (READ: the doc comment on `REDUX_TOOLKIT_RANGE`,
`crates/cratestack-client-typescript/src/rtk/deps.rs:48`, which records the measurement on the
published tarballs): `serializeQueryArgs` throws on a `bigint` before 2.2.4, and the generated
`rtk-api.ts` does not build with `declaration: true` before 2.2.7 (`TS2527`). The old `^2.0.0` floor
could not build at its lowest version. `packages/cratestack-adapter-rtk` still declares `^2.0.0` and
is a follow-up.

**What is pending, and why the release decision stands.**

- *B13.* A decision taken for PR B on 2026-10-09 requires the generated
  TypeScript client to be driven against a real server over JSON and `@cratestack/cbor-node`,
  including `@cratestack/link-batch`, a REST body, a REST query and the TanStack and RTK keys. It has
  not been written. What exists instead is a client-side suite (the generated client against a fake
  transport and the real revival and encode code, in `crates/cratestack-client-typescript/tests/js`)
  and the cross-language CBOR bytes pinned in every bridge. Dart stays on the pinned bytes.
- *PR C.* The `Int` cutover, `cratestack upgrade int-to-bigint`, snapshot format 3 and digest domains
  `v2`, exactly as §7 of the context doc lists. Until it lands `Int` is an `i64`, introspection maps
  `int8` to `Int`, and editing a field from `Int` to `BigInt` makes `migrate diff` emit an `ALTER`
  to the type the column already has, classed `Lossy` (READ: `emit_alter_column_type`,
  `ir.rs`).
- *Docs and skills.* The `cratestack-docs` pages and `cratestack-skills` entries of the context doc's
  §4.4 are not written.
- *Release.* Landing A and B on `main` is not a release. The reason B and C ship together (a release
  with `BigInt` and the old `Int` gives users two upgrades and makes a hand edit from `Int` to
  `BigInt` an `ALTER` to the same type) is unchanged, so 0.16.0 waits for C, as the Release decision
  says. The `## Unreleased` entry says plainly that `Int` is still `i64`.

## Consequences

### Positive

- No integer a `.cstack` schema can declare is corrupted in transit to JavaScript or Dart. `Int`
  cannot exceed what a JS number holds; `BigInt` never travels as a number.
- **One wire form on every codec (D2).** A `BigInt` is the same decimal string in a JSON body, a
  CBOR body, a batch frame and a projection, so a value decodes identically whichever codec a
  client negotiates, and CBOR, the primary codec for these projects, has a decision of its own.
- The grammar means what a Prisma user expects (`Int` 32-bit, `BigInt` 64-bit and a JS `bigint`),
  and `int4` columns finally have a scalar. vpay's schema documents widening `int4` columns to
  `BIGINT` only because `Int` could only mean `int8` (`schemas/vpay.cstack` on `origin/master`,
  the `Credential.counter` and `RateLimitWindow.attempts` comments, READ).
- A schema attribute either does something or fails `check`, at every position.
- The upgrade is mechanical and leaves storage untouched; the only schema-visible change is a
  rename the tool makes and verifies.

### Negative, and accepted

- **Every existing `Int` field changes wire form at the release.** After the codemod it is a JSON
  string, a TS `bigint` and a Dart `BigInt`. Clients and servers must be regenerated and deployed
  together; an old TS client reading a new server sees strings where it declared `number`. The
  generated revival throws on a number at a `BigInt` key rather than accept a value that may
  already be rounded.
- **Rust code that used the `i64` field now sees `cratestack::BigInt`** and converts with `get`
  and `new`. The compiler finds every site.
- **Narrowing costs a table rewrite under an `ACCESS EXCLUSIVE` lock**, per field, when the owner
  chooses it. The codemod never narrows.
- **There is no scalar for a 64-bit column that should be a JS `number`.** A counter that will
  never pass 2^53 but might pass 2^31 is a `BigInt`. That is the price of `Int` being exact by
  construction rather than by validation (see alternative B).
- **A `BigInt` in `cratestack_audit` JSON and `@@emit` payloads becomes a string**, because both
  serialize the model. Consumers that parse those payloads see the change.
- **Postgres `count(*)` and `sum(integer)` return `bigint`**
  ([aggregate functions](https://www.postgresql.org/docs/current/functions-aggregate.html)). A
  `query` block or `view` field typed `Int` over one fails to decode until it is typed `BigInt` or
  the SQL casts to `integer`.
- **The name `BigInt` means a 64-bit integer, not arbitrary precision** as `num_bigint::BigInt`
  does in Rust. It follows SQL's `BIGINT` and Prisma; the rustdoc says so.
- **CBOR payloads grow** by up to 12 bytes per `BigInt` value (21 bytes for `i64::MIN` as text
  against 9 as an integer). This is the price D2 pays for one wire form, accepted by the
  maintainer with CBOR as the primary codec.
- **D5 rejects schemas that parse today** if they carry an attribute nothing reads. The census
  found none; an unseen schema that has one learns about it at `check` time, which is the point.

### Downstream

Counted from parser spans and cross-checked by token count (RAN, E8). After the codemod every
field below is `BigInt`; "could narrow" counts `@version` counters, values bounded inside `i32` by
an `@range`, and documented small counts. The per-field table is the context doc's §8.

| Schema | `Int` references | Stay `BigInt` | Could narrow to `Int` |
|---|---|---|---|
| vpay `schemas/vpay.cstack` | 33 | 28 (money, `seq`, an HOTP counter) | 5 |
| skyport-billing `schema/skyport-billing.cstack` | 5 | 4 (including `UsageTally.amount_e8`) | 1 |
| tenant-provisioner `schema/provisioner.cstack` | 2 | 0 | 2 |
| tenant-provisioner `schema/spool.cstack` | 0 | 0 | 0 |
| vsms `schemas/vsms.cstack` | 56 | 2 (audit row counts) | 54 |

No downstream schema declares a name `BigInt` or a field attribute outside D5's union (RAN, E8,
E9). In this repository, 225 of the 273 committed `.cstack` files mention `Int` (RAN, E10); most
are width-agnostic fixtures that stay `Int`, and those whose tests depend on `i64` storage are
converted with the codemod itself in the implementing PR.

## Alternatives considered

**(B) Keep `Int` as a 64-bit column constrained to ±(2^53 - 1), and add `BigInt` for the full
range.** No DDL change, and existing clients keep `number`. Rejected: `Int` would be a Rust `i64`
that admits values its wire forbids, so correctness depends on validation at every boundary
(input, output, a database `CHECK`), and a row written by anything else turns a read into an
error. The range has a real pedigree, RFC 7493 (I-JSON, [§2.2](https://www.rfc-editor.org/rfc/rfc7493#section-2.2))
names it as the interoperable one for JSON numbers, but no column type has it, so the database
would hold values the type forbids, and a Dart VM or Rust client would carry a limit that exists
only for JavaScript. D1 makes `Int` exact by construction instead of by validation.

**(C) Keep `Int` as `i64` and change its JSON encoding to a string.** Fixes the precision for every
field, and adds no scalar. Rejected: it does not do what was asked (there is no `BigInt`), it turns
every counter, page size and version into a `bigint` in TypeScript with no way back to `number`,
and it leaves `int4` unmapped.

**`BigInt` as a native CBOR integer (major type 0 or 1).** Smaller, and what CBOR is for.
Rejected, and the maintainer confirmed it: the batch envelope's `serde_json::Value` frames would
put the JSON string on CBOR anyway, the Dart web codec would round or throw at its JS JSON-text
boundary, and both JS bridges would need changing (D2). Revisiting it means reworking those three
paths first, as its own ADR that supersedes D2, not a flag.

**`BigInt` as an RFC 8949 bignum (tag 2 or 3).** The CBOR-native form for large integers. Rejected
in D2: every `i64` already has a native integer form, so a bignum is a second, non-preferred
spelling of the same value ([RFC 8949 §3.4.3](https://www.rfc-editor.org/rfc/rfc8949#section-3.4.3),
[§10](https://www.rfc-editor.org/rfc/rfc8949#section-10)), and neither the batch frames, the Dart
JSON-text boundary nor the bridges' `cratestack_core::Value` has a place for a tag.

**A raw `i64` with a serde `with` attribute.** Keeps Rust ergonomics. Rejected in D3: it is wrong,
silently, on every path that does not see the field's attributes.

**Accept a JSON number for `BigInt` as well as a string, or only in the safe range.** Friendlier to
hand-written callers. Rejected: two accepted forms is the parallel path the maintainer's rule
excludes, and the server cannot tell an exact number from a rounded one.

**A per-field `@string` or `@wire(string)` attribute on `Int`.** Opt-in and additive. Rejected: the
default stays unsafe, and a field-level encoding switch is exactly the dual path D4 rules out.

**A codemod that keeps "obviously small" fields as `Int`** (`@version`, fields with an `@range` max
inside `i32`). Fewer wire changes. Rejected: every one of those is a `BIGINT` column today, so the
tool would be scheduling table rewrites on its own judgement. Narrowing stays a human edit.

**A deprecation release, or a flag selecting the old `Int`.** Rejected for the reasons ADR 0017
gives: the framework makes no additive-only promise pre-1.0, and a flag keeps paying for both
meanings.

**Keep #679's option (b) for field attributes.** Rejected: it is what let `@string` report
`schema OK`, and the reasons it was chosen (no derived set, five declaration kinds, blast radius)
are answered by GHSA-69g4-xvcm-vm2j's reader tables and by the census in E9.

## Related

- [`docs/design/int-and-bigint.md`](../design/int-and-bigint.md): per-target changes, diagrams,
  implementation plan (three PRs, all in 0.16.0, with the cross-language `i64::MAX`, `i64::MIN`
  and `2^53 + 1` round trips in Rust, TypeScript and Dart on both codecs), downstream table,
  evidence.
- [cratestack#679](https://github.com/cratestack/cratestack/issues/679) (field attributes, option
  (b) shipped in #810), GHSA-69g4-xvcm-vm2j (closed lists for the other positions, 0.14.1),
  [cratestack#1065](https://github.com/cratestack/cratestack/issues/1065) (digest precedent),
  [cratestack#1123](https://github.com/cratestack/cratestack/issues/1123) (per-op digests),
  [cratestack#1127](https://github.com/cratestack/cratestack/pull/1127) (binding v2, merged on
  2026-09-30 as `4c7e25cb`, after this worktree's base),
  [cratestack#1128](https://github.com/cratestack/cratestack/issues/1128) (`autoincrement()`,
  whose identity column type follows the scalar either way).
- ADR 0017 and ADR 0018 for the pre-1.0 compatibility posture this relies on.
- [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949), CBOR: §3.1 (major types), §3.4.3 (bignums)
  and §10 (security considerations), the basis for D2's CBOR text-string form (READ, 2026-09-30).
