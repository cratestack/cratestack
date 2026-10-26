# Explicit enum ordinals: a compact, append-only enum encoding

Status: **proposed** (2026-10-26). **Not implemented.** Nothing below exists in any release;
the docs guide [Reducing payload size](https://github.com/cratestack/cratestack-docs/blob/main/guides/payload-size.md)
links here and says so.

Scope: every place a `.cstack` enum value is stored, sent or parsed — the parser, the three
schema macros, the sqlx and rusqlite runtimes, `cratestack-migrate`, `cratestack diff`, both
codecs, JSON Schema and MCP, the Rust, TypeScript and Dart clients, WireMock, Studio, the
contract digests, and the LSP / tree-sitter / VS Code grammars.

**Evidence convention.** `path:line` anchors were READ on `claude/relaxed-curie-vs6j06` at
0.16.0 + Unreleased (2026-10-26). Byte counts were RAN with the probe in §8, which drives
`minicbor-serde` 0.7.1 configured exactly as `CborCodec::encode_into` configures it
(`crates/cratestack-codec-cbor/src/lib.rs:53-55`) and plain `serde_json`.

## 1. What an enum is today

An enum is a string everywhere it travels.

| Surface | Representation | Source |
|---|---|---|
| Rust type | unit variants, each `#[serde(rename = "<Variant>")]` | `crates/cratestack-macros/src/types/enums.rs` `variant_tokens` |
| SQL bind | `SqlValue::String(self.to_string())` | `types/enums.rs`, both `IntoSqlValue` impls |
| Postgres column | `TEXT` + `CHECK (col IN ('A', 'B'))`, no `CREATE TYPE` (#228) | `crates/cratestack-migrate/src/emit/postgres/columns.rs:169-176`, `emit/postgres/checks.rs:35` |
| SQLite column | declared `BLOB` like every SQLite column, the value stored as text, plus the membership `CHECK` | `crates/cratestack-migrate/src/emit/sqlite/columns.rs:3`, `emit/sqlite/checks.rs` |
| CBOR / JSON body | text string | RAN, §8 |
| TypeScript | `export type E = 'A' \| 'B'` plus `EValues` | `crates/cratestack-client-typescript/templates/src/models.ts.j2:521-527` |
| Dart | `enum E { a('A'), … }` with `String wireName`, `fromWire` throws `ArgumentError` on an unknown value | the pattern `SortDirection` shows in `crates/cratestack-client-dart/templates/models.dart.j2:116-136` |

What that costs, per value, on the wire:

| Value | CBOR | JSON |
|---|---|---|
| `PendingReview` as today | 14 B | 15 B |
| the same as a small integer (≤ 23) | 1 B | 1 B |
| the same as an integer 24–255 | 2 B | 2–3 B |

And in Postgres: a `TEXT` value of *n* ASCII characters is *n* + 1 bytes (short varlena
header), so `PendingReview` is 14 B against 2 B for a `SMALLINT`, in the heap and again in every
index that carries the column.

On a realistic list response — 500 rows, 5 fields, two of them enums, varied data (§8) — the
saving is real uncompressed and nearly gone once the body is gzipped:

| 500 rows | CBOR | CBOR + gzip | JSON | JSON + gzip |
|---|---|---|---|---|
| today | 59 411 | 18 674 | 79 409 | 21 236 |
| two enums as ordinals | 47 438 (−20 %) | 18 429 (−1.3 %) | 66 436 (−16 %) | 20 715 (−2.5 %) |

Ordinals are therefore worth having for **uncompressed hops** (mobile clients without
`Content-Encoding`, `application/cbor-seq` streams, the outbox, signed envelopes whose payload
is sealed before any HTTP compression could see it), for **storage and index size**, and for
**stability** (§3) — not as the first lever for a gzipped JSON API. The docs guide ranks the
levers in that order.

### 1.1 Two defects this design also closes

1. **`cratestack diff` does not look at enums at all.** No file under
   `crates/cratestack-cli/src/schema_diff/` reads `Schema::enums`. Removing or renaming a
   variant is a wire-breaking change for every client, and the API-break detector reports
   nothing. This is true today, independent of ordinals. §3.3 adds the check for both kinds of
   enum.
2. **The obvious shortcut is a trap.** "Store the variant's declaration index" is one line of
   codegen and silently remaps every stored row the first time someone sorts the variants
   alphabetically or inserts one in the middle. This design refuses implicit ordinals outright:
   an ordinal is written down, or there is none.

## 2. Proposal

### 2.1 Syntax

```cstack
enum PaymentStatus {
  Pending = 1
  Approved = 2
  Rejected = 3
  Refunded = 5

  @@reserved(4)   // was Settled; retired 2026-11 — never reuse
}
```

An enum whose variants carry `= <n>` is an **ordinal enum**. An enum without them is unchanged:
strings everywhere, exactly as §1. Existing schemas do not move.

Parse-time rules (single schema, no history needed):

- **R1 — all or none.** If one variant has an ordinal, every variant must. A partially numbered
  enum is refused, naming the unnumbered variants.
- **R2 — range and uniqueness.** An ordinal is a decimal integer literal in `0..=32767` (fits
  `SMALLINT` and a one-to-three-byte CBOR integer), unique within the enum.
- **R3 — reserved means reserved.** `@@reserved(n, …)` lists retired ordinals; a variant may not
  use one. `@@reserved` on a non-ordinal enum is refused.
- **R4 — closed attribute list.** `@@reserved` is the only block attribute an enum accepts,
  added to the closed lists of ADR 0019 D5 (`cratestack_parser::field_attribute_names` and the
  block-attribute allowlist), so a misspelling is a refusal, not a silent no-op.

Why `= n` and not `@value(n)`: the number is the variant's identity on disk and on the wire, not
a modifier on it; it reads the way Rust discriminants and protobuf field numbers read, which is
what a reviewer will pattern-match against. The cost is grammar work in three places — the
chumsky parser, the tree-sitter grammar and the VS Code TextMate grammar — where `@value(n)` would
have reused the attribute path. Open question O4 records the alternative.

### 2.2 What each surface does with an ordinal enum

Variant **names** stay the authoring and application surface everywhere a human types or reads
one. The **ordinal** is what is stored and sent.

| Surface | Ordinal enum |
|---|---|
| Rust type (server and client) | `#[repr(i16)]` with the declared discriminants. `Serialize`/`Deserialize` hand-emitted as an integer (no new dependency). `as_str`, `Display`, `FromStr` keep using names. New: `const fn ordinal(self) -> i16` and `TryFrom<i16>`. |
| SQL bind | `SqlValue::Int(self.ordinal() as i64)` |
| Postgres | `SMALLINT` + `CHECK (col IN (1, 2, 3, 5))`; row decoders `try_get::<i16>` then `TryFrom` |
| SQLite | still a `BLOB`-declared column, the value stored as an integer, plus the same `CHECK` |
| CBOR / JSON body | integer |
| REST query string (`?status=Approved`, `__in`, sort) | **names**, unchanged. The URL is the human-facing surface; codegen maps name → ordinal before binding. |
| RPC | frames carry integers like REST bodies; filters reach the REST parser through `cratestack-axum/src/rpc/synthesize.rs`, so filter parity is free |
| Policy literals (`auth().role == admin`) | names in source; `policy/model/enum_literal.rs` resolves to the ordinal at codegen |
| `@default(Pending)` | name in source; DDL emits `DEFAULT 1` |
| JSON Schema / MCP tool input | `{"type": "integer", "oneOf": [{"const": 1, "title": "Pending"}, …]}` so an agent calling a tool still sees names |
| TypeScript | app-facing type **unchanged**: `'Pending' \| 'Approved' \| …`. Revival maps ordinal → name on decode and name → ordinal on encode; new `EOrdinals` table. Application code does not change when an enum becomes ordinal. |
| Dart | enum constants unchanged; `String wireName` becomes `int wireValue`; `fromWire` switches on the integer |
| WireMock stubs | integers |
| Studio | renders the name from the schema, with the stored integer alongside |
| Contract digests | the enum's wire form is part of the op contract, so making an enum ordinal changes every digest that reaches it — correctly, it is a wire break |

**Transport parity** (CLAUDE.md): REST, RPC and every generated client (Rust/Dart/TS, swr and
riverpod layers) ship in the same PR. MCP follows ADR 0002 D5 and gets the JSON Schema change
because it consumes the same schema generator, not because parity requires it.

### 2.3 Decoding is tolerant, encoding is strict

Every decoder of an ordinal enum — server request bodies, generated clients — accepts **either**
the integer or the variant's name string, and every encoder emits the integer. This costs one
extra match arm per decoder and buys a rollout in which a client still sending names keeps
working (§4). An unknown integer is a decode error, exactly as an unknown name is today
(Rust serde error; Dart `ArgumentError`; TypeScript revival error). Forward compatibility for
values a newer server adds is a separate question (O1) and is not solved by this design.

## 3. Enforcement: the ordinal ↔ variant map is append-only

This is the part the feature exists for. A stored row means what its ordinal meant on the day it
was written, forever.

### 3.1 Parse time

R1–R4 above.

### 3.2 `cratestack migrate diff` (sees the previous snapshot)

Against the last committed `schema.snapshot.json`:

| Change | Verdict |
|---|---|
| new variant, new ordinal | allowed; `CHECK` widened |
| variant renamed, ordinal kept | allowed and **storage-neutral** — no data moves; `CHECK` unchanged |
| a variant's ordinal changed | **refused**: "ordinal of `PaymentStatus.Approved` changed 2 → 7; ordinals are permanent. Add a new variant instead." |
| variant removed, its ordinal not added to `@@reserved` | **refused**, naming the ordinal to reserve |
| variant removed and reserved | allowed; `CHECK` narrowed — **destructive** (rows holding it fail the new `CHECK`), so it goes through the existing destructive classification and `up.pre.sql` path |
| a previously used ordinal reused by a different name | **refused** |
| `@@reserved` entry removed | **refused** |

The snapshot only remembers one step back, which is why retirement has to be written in the
schema (`@@reserved`) rather than inferred: a number freed three migrations ago is invisible to
the differ otherwise.

### 3.3 `cratestack diff` (API break detector)

Adds enum diffing, for **both** kinds of enum (defect 1.1.1):

| Change | Severity |
|---|---|
| variant removed | breaking |
| variant renamed (string enum) | breaking — the wire value changed |
| variant renamed (ordinal enum) | wire-neutral; flagged as client-source-breaking (generated identifiers change) |
| variant added | non-breaking for the server; warned, because a client that decodes exhaustively rejects the new value until regenerated (O1) |
| string enum → ordinal enum | breaking |
| ordinal changed | breaking (and already refused by 3.2) |

### 3.4 Tests the implementing PRs must carry

- parser: each of R1–R4 refused with its span; a valid ordinal enum round-trips through
  `print-ir`.
- migrate: every row of §3.2's table, both dialects; the string → ordinal conversion of §4 on a
  table with data (PG-backed, `CRATESTACK_REQUIRE_DB=1`).
- macros: server and client `Serialize` emit integers on both codecs; `Deserialize` accepts the
  integer and the name; an unknown integer is a decode error.
- clients: TS revival maps both directions; Dart `fromWire(int)` and `fromWire(String)`; the
  committed example clients regenerate (`just regen-examples`).
- `cratestack diff`: every row of §3.3.

## 4. Converting an existing string enum

`migrate diff` knows both mappings, so it emits the conversion rather than leaving it to hand
SQL (Postgres):

```sql
ALTER TABLE payments DROP CONSTRAINT payments_status_enum_check;
ALTER TABLE payments ALTER COLUMN status TYPE SMALLINT USING
  CASE status WHEN 'Pending' THEN 1 WHEN 'Approved' THEN 2
              WHEN 'Rejected' THEN 3 WHEN 'Refunded' THEN 5 END;
ALTER TABLE payments ALTER COLUMN status SET DEFAULT 1;
ALTER TABLE payments ADD CONSTRAINT payments_status_enum_check CHECK (status IN (1, 2, 3, 5));
```

It is classified **destructive**: it rewrites the table under an `ACCESS EXCLUSIVE` lock, and a
stored value outside the mapping becomes `NULL` and then fails `NOT NULL`. SQLite needs its usual
table rebuild.

On the wire it is a breaking change, and §2.3 narrows the window rather than closing it:

1. Regenerate and ship clients against the ordinal schema. Their decoders accept names *and*
   integers, so they keep working against the old server — but they *send* integers, which the
   old server rejects. So in practice step 1 and step 2 deploy together, or:
2. Deploy the server. It accepts names from clients not yet updated, and answers with integers,
   which those clients cannot read.

Clients that cannot be updated in lockstep (installed mobile apps) need the enum change to ride
an `@api_version` bump, so old clients keep calling the old shape. That is the honest answer; no
encoding trick avoids it.

## 5. The debuggability cost

The argument for ordinals is that the schema is the decoder: an agent or a human with the
`.cstack` file in hand can read `status = 3` as `Rejected`. That holds exactly where the schema
travels with the data, and not elsewhere:

| Reader | Sees |
|---|---|
| generated clients, Studio | names (§2.2) |
| `psql`, a BI tool, a backup restored years later | integers |
| `@@audit` before/after snapshots, outbox payloads | integers, if they serialize the model's wire form |
| logs that print a `Debug` of the Rust type | names (`#[derive(Debug)]` is unchanged) |

Mitigation is O3. The recommendation in the docs guide follows from this table: ordinals for
high-volume, stable, machine-read enums (event kinds, statuses on hot tables); names for small
tables and anything a regulator or an on-call engineer reads raw.

## 6. Out of scope, recorded so nobody re-derives it

- **`DateTime` on the wire.** It is `TIMESTAMPTZ` (8 B) in Postgres, but an RFC 3339 text string
  on **both** codecs — 21 B with whole seconds, 29 B with microseconds, 32 B with nanoseconds
  (RAN, §8): `chrono`'s `Serialize` does not branch on
  `is_human_readable()`, unlike `uuid::Uuid`, which does take a 16-byte binary branch on CBOR
  (17 B against 38 B in JSON). As an `i64` epoch it would be 5–9 B. A CBOR-only compact form
  (RFC 8949 tag 1, or RFC 9581 tag 1001 for exact sub-second precision) is a separate design with
  its own client-decoder work; it is the second-largest uncompressed saving after field names.
  The embedded backend stores `DateTime` values as RFC 3339 text, so there the value *is* a string at rest.
- **Field names.** Every map key is repeated per row: `id`, `status`, `createdAt`,
  `totalMinor` are 33 of a 66-byte CBOR row. No attribute renames a field on the wire only
  (`@rename(from = "…")` is a migration marker), and this design does not add one.
- **Compression.** Not a framework feature: the generated router is a plain `axum::Router`, and
  `tower-http`'s `CompressionLayer` goes on it like any other layer.

## 7. Open questions

- **O1 — unknown values.** Should an enum be able to declare a fallback (`@@open`, decoding an
  unknown ordinal to an `Unknown(i16)` variant) so that adding a variant stops being a client
  break? Applies to string enums equally; deliberately not folded in here.
- **O2 — digits in the query string.** Should `?status=2` be accepted alongside `?status=Approved`?
  Proposed: no, one spelling per surface.
- **O3 — a lookup relation in the database.** Should `migrate` emit
  `cratestack_enum_payment_status(ordinal SMALLINT PRIMARY KEY, name TEXT)` (and keep it in sync)
  so raw SQL can `JOIN` names back? Cheap to emit, one more object to migrate.
- **O4 — syntax.** `Pending = 1` (proposed) or `Pending @value(1)`.
- **O5 — default.** Ordinal stays opt-in. Making it the default for new enums would make the
  first-time author choose numbers before they know which enums are hot; not proposed.

## 8. Measurements

RAN on 2026-10-26, `minicbor-serde` 0.7.1, `chrono` 0.4, `uuid` 1, `rust_decimal` 1 with
`serde-str` (the workspace's feature set), `serde_json` 1, `flate2` default level for gzip. The
enum-as-integer rows use `serde_repr` to stand in for the hand-emitted impl of §2.2.

Single values (`cbor` / `json` bytes):

```text
DateTime (chrono), microseconds  cbor= 29B json= 29B   cbor_hex=781b3230…   (text string)
DateTime as i64 epoch seconds    cbor=  5B json= 10B
DateTime as i64 epoch millis     cbor=  9B json= 13B
Uuid                             cbor= 17B json= 38B   cbor_hex=50…         (byte string)
enum unit variant "PendingReview" cbor= 14B json= 15B
enum as integer 3                cbor=  1B json=  1B
Decimal 1234.56                  cbor=  8B json=  9B   (text string, serde-str)
bool                             cbor=  1B json=  4B
```

500 rows of `{ id: Uuid, eventType, status, createdAt: DateTime, amount: Decimal }`, five
event types and five statuses drawn pseudo-randomly, timestamps spread over a day:

```text
A today: string type+status        cbor= 59411 cbor+gz= 18674 json= 79409 json+gz= 21236
B ordinal enums                    cbor= 47438 cbor+gz= 18429 json= 66436 json+gz= 20715
C ordinal enums + epoch-us time    cbor= 37438 cbor+gz= 16231 json= 59936 json+gz= 19461
```

The probe's encoder, so the numbers can be reproduced against a later `minicbor-serde`:

```rust
fn cbor<T: serde::Serialize>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    let mut s = minicbor_serde::Serializer::new(&mut out);
    s.serialize_unit_as_null(true); // as CborCodec::encode_into
    v.serialize(&mut s).unwrap();
    out
}
```
