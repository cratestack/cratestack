# ADR 0019: `Int` and `BigInt` as built-in integer types

## Status

Proposed

> **Placement note.** `docs/adr/README.md` sends decisions about the user-visible surface
> (`.cstack` grammar, transport semantics, migration behaviour) to `cratestack-docs/internals/`.
> This one changes all three. It is filed here, as ADR 0018 was, because it was asked for in this
> repository and because its argument is made of crate-internal paths from start to finish. Move
> it if the maintainer reads the split the other way. 0019 is the next free number in both
> repositories: `cratestack-docs/internals/` holds 0001 to 0006, 0007 to 0010 are reserved, and
> this directory ends at 0018.

## Date

2026-09-30 (proposed, this PR). Decision requested by the maintainer on 2026-09-30: "Fix
CrateStack. Propose Int and BigInt as built-in types."

Context doc: [`docs/design/int-and-bigint.md`](../design/int-and-bigint.md), which carries the
per-target changes with `path:line` citations, the sequence and state diagrams, the implementation
plan and the evidence. Every `path:line` here and there was READ on `origin/main` at `67100917`
(v0.15.1; `main` has since gained #1127 at `4c7e25cb`, which shifts a few of those lines); every
behaviour marked RAN was executed for this ADR and is reproduced in the context
doc's §9 under the key given (E1 to E11).

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

**D2. A `BigInt` travels as a canonical decimal string on every codec, and only that form is
accepted.** `0`, or an optional `-`, a non-zero digit and up to 18 more digits, inside `i64`.
Emitted by `Display`, so the server only writes the canonical form. A JSON number, a CBOR
integer, `+5`, `007`, `-0` and anything outside `i64` are refused with a message naming the
field (prototype RAN, E5). A number is refused because a value above 2^53 may already have been
rounded by the JavaScript that produced it, and the server cannot tell; that is also the rule
`Decimal` follows today (`rust_decimal` with `serde-str`, root `Cargo.toml:405`, refuses a JSON
number; RAN, E5). The same string on CBOR, rather than a native integer, because the batch
envelope carries frames as `serde_json::Value` (`crates/cratestack-core/src/rpc.rs:107`, `:118`),
because the `cratestack_cbor` Dart codec crosses a JSON-text boundary on both its platforms, and
through JS `JSON.parse` on the web (`dart-packages/cratestack_cbor/lib/src/cbor_codec.dart:17-31`,
`lib/src/web/web_cbor_codec.dart:75`, `:95`), because the two JS bridges disagree about large
integers today, and because `Decimal` already does it (RAN: `1.50` encodes as `64312e3530`, E5).
A string needs none of those paths changed. The cost is size: 20 bytes for `i64::MAX` against 9.

**D3. The Rust type is a newtype, `cratestack::BigInt`, in `cratestack-core`.** Its `Serialize`
writes the decimal string and its `Deserialize` accepts only D2's form. A raw `i64` with a
`#[serde(with = ...)]` attribute was the alternative, and it fails silently on every path that
does not see the struct field's attributes: `?fields=` projections serialize each field as a
type-erased leaf (`crates/cratestack-macros/src/axum/model/serializers/projection_fields.rs:24`,
`crates/cratestack-axum/src/projection.rs:72`), a bare procedure return is the value itself,
`FieldFilterInput<T>` is generic, and audit snapshots are `serde_json::to_value(&record)`
(`crates/cratestack-sqlx/src/query/write/create.rs:92`). With a newtype each of those is right
by construction, and a place that still expects `i64` is a compile error. The database boundary
converts in generated code (`try_get::<i64>` then `BigInt::new`; `SqlValue::BigInt(i64)`), so the
newtype needs no sqlx or rusqlite impls and `cratestack-core` keeps zero workspace dependencies.
It has `new`, `get`, `From<i64>`, `Display`, `FromStr` and the comparison traits, and no
arithmetic operators: money arithmetic stays explicit on `i64`.

**D4. The change ships as one breaking release, with a codemod and no compatibility path.**

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

**D5. Field attributes become a closed list, per declaration kind.** The companion decision, and
#679's option (a). Each of `model`, `view`, `mixin`, `type` and `auth` gets a table of the field
attributes it accepts, checked by the same `check_shape`
(`crates/cratestack-parser/src/validate/attribute_shape.rs:38`) that already closes block,
procedure and query attributes, so an unknown name, another case or a stray argument list is an
error, with a suggestion when a known name is close. The union the readers use today is 19 names (context doc §5), and no
field attribute outside it appears in any of the 265 committed schemas that parse or in the five
downstream schemas (RAN, E9). That census is the migration story #679 asked for, and the answer to
the objection recorded at `misspelled_attributes.rs:17-25`. This is what turns `@string` from a
silent no-op into the error that sends its author to `BigInt`. It is independent of D1 to D4 and
can ship in any release.

**How this honours the request.** `Int` and `BigInt` are both built-in, first-class scalars with
their own type on every target, their own column type, their own filter type and their own wire
form; neither is an attribute, an alias or a flag on the other, and the old meaning of `Int` does
not survive the release anywhere.

## Consequences

### Positive

- No integer a `.cstack` schema can declare is corrupted in transit to JavaScript or Dart. `Int`
  cannot exceed what a JS number holds; `BigInt` never travels as a number.
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
  against 9 as an integer).
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

**`BigInt` as a native CBOR integer.** Smaller, and what CBOR is for. Rejected for now: the batch
envelope's `serde_json::Value` frames would put the JSON string on CBOR anyway, the Dart web codec
would round or throw at its JS JSON-text boundary, and both JS bridges would need changing (D2).
Revisiting it means reworking those three paths first, as its own ADR, not a flag.

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
  implementation plan (three PRs, with the cross-language `i64::MAX`, `i64::MIN` and `2^53 + 1`
  round trips in Rust, TypeScript and Dart on both codecs), downstream table, evidence.
- [cratestack#679](https://github.com/cratestack/cratestack/issues/679) (field attributes, option
  (b) shipped in #810), GHSA-69g4-xvcm-vm2j (closed lists for the other positions, 0.14.1),
  [cratestack#1065](https://github.com/cratestack/cratestack/issues/1065) (digest precedent),
  [cratestack#1123](https://github.com/cratestack/cratestack/issues/1123) (per-op digests),
  [cratestack#1127](https://github.com/cratestack/cratestack/pull/1127) (binding v2, merged on
  2026-09-30 as `4c7e25cb`, after this worktree's base),
  [cratestack#1128](https://github.com/cratestack/cratestack/issues/1128) (`autoincrement()`,
  whose identity column type follows the scalar either way).
- ADR 0017 and ADR 0018 for the pre-1.0 compatibility posture this relies on.
