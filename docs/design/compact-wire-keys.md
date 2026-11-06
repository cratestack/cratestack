# Compact wire keys: integer field keys selected by the op contract

Status: **proposal** (2026-11-06). Not accepted; no code. This doc compares four ways to stop
repeating field names in CBOR bodies when client and server share the `.cstack` schema, and
recommends one (§4). Nothing here changes the wire today.

Scope: CBOR request and response bodies of REST routes and RPC ops (unary, batch frames, WS
frames, `application/cbor-seq` items), and the Rust, Dart and TypeScript clients that encode and
decode them. JSON, query strings, error bodies, WireMock, MCP and Studio stay named (§5.5).

**Evidence convention.** `path:line` anchors were READ on `claude/relaxed-brahmagupta-krvnpc`
at `801d14d` (2026-11-06). Byte counts were RAN with the probe in §8, which drives
`minicbor-serde` 0.7.1 configured as `CborCodec::encode_into` configures it
(`crates/cratestack-codec-cbor/src/lib.rs`), `serde_json`, `flate2` and `zstd`.

## 1. What a body is today

A generated struct is a `#[derive(Serialize, Deserialize)]` with camelCase renames, so on CBOR it
is a map whose keys are **text strings**, one per field, repeated in every object. RPC op ids
(`model.Product.create`, `procedure.transferFunds`) are text too: in the URL for unary calls, in
the frame's `op` slot for batch and WS frames (`docs/design/rpc-transport.md` §2.1, §2.3).

Enum values already have a compact form (`docs/design/enum-ordinals.md`). Field names do not,
and that doc records it as out of scope (§6 there: "No attribute renames a field on the wire
only … and this design does not add one").

## 2. Where the bytes go

RAN (§8). CBOR bytes. *gz* is gzip at flate2's default level, *zstd+dict* is zstd level 3 with a
raw-content dictionary of the schema's identifiers, and *+dcz* adds the 40-byte framing
Compression Dictionary Transport puts in front of every `dcz` body.

| Payload | names (today) | gz | zstd+dict | +dcz | integer keys | positional array | JSON |
|---|---|---|---|---|---|---|---|
| createProduct request | 205 | 207 | 163 | 203 | **133** (−35 %) | 124 | 264 |
| createProduct response | 312 | 277 | 226 | 266 | **213** (−32 %) | 200 | 404 |
| transfer request | 123 | 146 | 87 | 127 | **65** (−47 %) | 60 | 183 |
| transfer response | 242 | 253 | 164 | 204 | **132** (−45 %) | 121 | 346 |
| 500-row list | 47 465 | 15 702 | 15 761 | 15 801 | 31 465 (−34 %) | 28 965 | 66 463 |

500-row list with integer keys, gzipped: 15 321 B (−2.4 % against 15 702 for names, gzipped).

Read it in two directions:

- **Single calls are not helped by compression.** A procedure body is a few hundred bytes. Gzip
  makes three of the four bigger, and the dictionary transport's framing cancels what the
  dictionary saves. Most procedure traffic has no compression lever at all. **Integer keys
  remove 32–47 %** with no dependency and no negotiation round trip. What is left is mostly
  values: UUIDs and decimal strings.
- **Lists are already handled by compression.** Gzip removes two thirds of a list, and integer
  keys then save another 2.4 %. For lists, integer keys only matter on hops nothing compresses:
  `application/cbor-seq` streams, sealed envelopes, outbox rows, and mobile clients without
  `Content-Encoding`. That is the profile `enum-ordinals.md` §1 found for enums.

Positional arrays save another 4–8 % over integer keys. They are not proposed: absent optional
fields, tolerant decoding (§4.3) and dropping unknown fields (§4.4) all need a key.

Op ids: a batch frame carrying the transfer request with integer keys is 100 B with
`op: "procedure.transferFunds"` and 78 B with the op as an integer. So the op id is about 22 B
per frame, and in a unary call it sits in the URL, not the body. §5.6 leaves op ids alone.

## 3. The constraint: "the server is newer"

Client and server sharing one `.cstack` is the easy case. The real case is a server deployed
ahead of clients it cannot update in lockstep, such as installed mobile apps. Any integer key
scheme has to answer one question: **which numbering did this client use?**

The tempting answer is "the field's position in the declaration". Used blind, that is the trap
`enum-ordinals.md` §1.1.2 refuses for enums. It silently remaps every key the first time someone
inserts a field or sorts them. A key scheme is safe only if the numbering is pinned to something
both sides can name. There are three candidates, and a fourth option avoids numbering altogether.

## 4. Options

### A. Whole-schema exact match

The client already sends `x-cratestack-schema-sha` on every call
(`crates/cratestack-axum/src/schema_fingerprint.rs`). The server would use integer keys only when
that value equals its own `SCHEMA_SHA256`, and names otherwise.

This needs no schema ceremony. But `schema_digest` hashes the whole IR, so any edit drops every
existing client back to names, including a policy edit, an `@@index`, or a new procedure.
Compact keys would then only work in the window between two deploys.

### B. Explicit, append-only field numbers

This is protobuf's answer, and the shape of enum ordinals: `name String = 3`, with
`@@reserved(…)`, plus `migrate diff`-style refusals of renumbering and reuse. Numbers survive any
additive change, with no negotiation.

It costs a number on every field of every wire type, chosen before the author knows which types
are hot, and a second append-only ledger to enforce. It is also, almost literally, `@pb(N)` with
its field-number lockfile, which ADR 0017 removed (§6).

### C. Contract-keyed implicit numbering (recommended)

The framework already has a per-op identity that moves only when that op's wire shape moves, and
a mechanism for a server to accept older ones:

- **Per-op contract digests** (`crates/cratestack-core/src/client_contract.rs`, cratestack#1123).
  An op's contract is its input and output roots plus every model, type, enum and view reachable
  from them, in wire projection, with `@server_only` fields removed. A schema edit elsewhere does
  not move it.
- **Field order is already fixed by the contract.** `wire_fields` sorts a type's wire fields by
  name (`crates/cratestack-core/src/client_contract/project.rs:30-34`), so reordering fields does
  not move the digest. It follows that "index in that sorted list" is a numbering *both sides can
  recompute from the digest's contract alone*.
- **Clients already hold their digests.** `OP_CONTRACTS` is generated for Rust
  (`crates/cratestack-client-rust/src/client/contract.rs`), TypeScript (`rest-runtime.ts.j2`,
  `rpc-runtime.ts.j2`) and Dart (`constants.dart.j2`).
- **The selector header already exists.** `Cratestack-Contract` carries the first 8 bytes of the
  op's digest, unbound and untrusted, to pick among digests the server already accepts
  (`crates/cratestack-core/src/codec/contract_selector.rs`). Today only sealed calls send it.
- **The server already knows older contracts.** `ContractLock` stores, per older digest, that
  op's **full canonical contract** (`client_contract/lock.rs`, `contracts: BTreeMap<String,
  Value>`). `include_server_schema!`'s `contracts =` argument turns the compatible ones into
  `ACCEPTED_CONTRACTS` (`client_contract/table.rs`). §5.4 relies on the classifier's rules
  (`client_contract/compat.rs`). In the input direction, every field an older client sends still
  exists. In the output direction, the only additions it allows are ones a decoder ignores.

So the proposal is: **a key table is a pure function of an op contract.** For each type in the
closure, key *n* is the *n*-th wire field in name order. The client names its digest in
`Cratestack-Contract`. The server looks the digest up in `ACCEPTED_CONTRACTS` and uses that
contract's table, which codegen derived at compile time from the lock entry. Numbers never need
to be stable across versions, because every version's table is addressed by its own digest. A
newer server serves any client the contract classifier already calls compatible, and it serves
names to every other client.

There is no schema syntax and no second ledger. Without a contract lock it degrades to
exact-match per op, which is still much better than A: an edit to one op, or a server-only edit,
leaves every other op compact.

### D. Schema-derived compression dictionary

Build a zstd dictionary from the schema's identifiers, ship it to both sides, and send bodies as
`Content-Encoding: dcz`. The dictionary is addressed by hash, so a mismatch falls back to plain
compression. The wire shape does not change, and JSON benefits too.

§2 rules it out as the main lever. On single calls the 40-byte framing cancels the gain, and on
lists plain gzip already does almost all of the work. It would also add a zstd dependency to the
Dart and TypeScript runtimes, and browser support is Chromium-only. It stays open for streams
(O4).

### Comparison

| | A | B | C | D |
|---|---|---|---|---|
| Single-call saving | 32–47 %, only while digests match | 32–47 % | 32–47 % | ~0–16 % |
| Server newer than client | names | compact | compact, if the lock accepts the client's contract | compact |
| Schema syntax | none | a number per field + `@@reserved` | none | none |
| New ledger to enforce | none | yes | none (reuses `ContractLock`) | none |
| New runtime dependency | none | none | none | zstd (Dart, TS) |
| Relation to ADR 0017 | — | re-adds `@pb(N)` | independent (§6) | — |

## 5. Proposal C in detail

### 5.1 Negotiation

- A client opts in per call by sending the media-type parameter
  `application/cbor; keys=contract` in `Content-Type` (for its request body) and `Accept` (for the
  response), plus `Cratestack-Contract` for the op. Generated clients would send both when the
  call's codec is CBOR.
- The server decodes a compact request with the table of the selected digest. It answers compact,
  marked with the same media-type parameter, only when the request asked and the selector names an
  accepted digest. Otherwise it answers with names and plain `application/cbor`. A server never
  sends integer keys to a client that did not ask, so no client breaks.
- Responses carry `Vary: Accept, Cratestack-Contract`.
- A selector that names no accepted digest is **not** an error on an unsigned call: the server
  falls back to names. The `426 contract_unsupported` refusal stays specific to sealed calls.

### 5.2 Key table derivation

For every model, type and view in the op's contract closure, the wire fields in the order
`wire_fields` already produces, numbered from 0. Keys are CBOR unsigned integers, so 1 B up to 23
fields and 2 B up to 255. The RPC frame and input structs the framework owns (`RpcListInput`,
`RpcGetInput`, the batch frame) keep text keys in v1 (O2).

### 5.3 Decoding is tolerant, encoding is strict

As for enum ordinals (`enum-ordinals.md` §2.3), a decoder in compact mode accepts an integer key
**or** the field's name, and an encoder in compact mode emits integers only. That lets a client
roll out field by field, and it gives hand-written callers an escape hatch.

### 5.4 Fields the client's contract does not have

A server newer than its client may have response fields the client's table does not number. The
encoder **drops** them: the client could not decode them anyway, and generated decoders already
ignore unknown keys (generated types do not use `deny_unknown_fields`, `crates/cratestack-macros/
src/json_schema/parts.rs:20`). In the other direction, the contract classifier only accepts an
older contract if the server can read what that client sends, so a compact request always maps.

### 5.5 What stays named

- **JSON.** JSON object keys are strings, so `"3"` saves less and reads worse. Compact keys are
  CBOR-only.
- **Query strings, error bodies (`RpcErrorBody`), WireMock stubs, MCP tool I/O, Studio, and
  logs.** These are the human-facing surfaces.
- **Outbox and audit payloads.** They outlive the contract that wrote them, so they stay named
  unless O3 decides otherwise.

### 5.6 Op ids stay text

They are 22 B per batch or WS frame, nothing in a unary body, and they are the dispatch key that
rate limiting, idempotency (`crates/cratestack-axum/src/idempotency/rpc_op_resolver.rs`), signed
path binding and WireMock all match on. A per-op integer could be derived from the same contract
table later; it is not worth the churn now.

### 5.7 Interactions to settle before building

- **Idempotency.** `hash_request` hashes the content type and the body bytes
  (`crates/cratestack-axum/src/idempotency/hash.rs`). A retry that switches between compact and
  named for the same `Idempotency-Key` therefore reads as a conflict. Rule: a client never
  switches key mode within one idempotency key.
- **Signed envelopes.** A sealed body is sealed in its compact form. The AAD already binds the op
  contract digest, which fixes the table, but it must also bind the key mode. Otherwise the same
  bytes could be presented as a different encoding. That means a payload-type change
  (`crates/cratestack-core/src/codec/payload_type.rs`) and a binding-version note in ADR 0006
  (cratestack-docs `internals/cose-envelope-adr.md`).
- **Projections.** `?fields=` responses go through `ProjectedValue`
  (`crates/cratestack-axum/src/projection.rs`), which builds maps by field name at runtime. It
  needs the table too. A projected field keeps its full-contract number, so a projection is a
  subset of keys, not a renumbering.
- **Batch.** `/rpc/batch` re-encodes each frame's `input` (`include/server/rpc_module/batch.rs`).
  The table is per frame (per op), so the re-encode must carry the frame's selector. Today a
  signed batch binds the whole-contract digest. Batch frames should therefore stay named in v1,
  unless each frame carries its own selector.
- **Streams and subscriptions.** `cbor-seq` items and WS frames are where the uncompressed saving
  matters most. They take the table once per stream, from the opening request.

### 5.8 Implementation sketch (per surface)

- **Rust, server and client.** A `serde` adapter around `minicbor-serde`'s serializer and
  deserializer. On `serialize_struct` and `serialize_field` it swaps the field's name for its key
  from a generated `&'static` table, keyed by struct name and field name. On decode it maps an
  integer back to the name and hands it to the derive's field visitor. There is no change to the
  generated structs, and this stays one module in `cratestack-codec-cbor`, under the 200-LoC
  ceiling. Codegen emits one table per accepted digest.
- **TypeScript.** The RPC runtime's CBOR path already revives per type (`wireShapes` beside
  `ordinalEnumNames`), so the remap goes there. The REST runtime is JSON-only and has no codec
  seam, so it gets nothing until it speaks CBOR. That alone shrinks a transfer request from 183 to
  123 B, before any key change (§2).
- **Dart.** Generated `fromJson`/`toJson` work on `Map<String, dynamic>`. Codegen would emit a
  second pair keyed by integer from the same table, chosen by the HTTP runtime's codec.
- **Transport parity** (CLAUDE.md). REST and RPC ship together, with every generated client and
  the swr, riverpod and fespalier layers. MCP is exempt (ADR 0002 D5) and stays JSON in any case.

## 6. Relation to ADR 0017

ADR 0017 removed `transport grpc`, including `@pb(N)` and its field-number lockfile. Option B
would re-add that mechanism under another name. Option C does not:

- It adds no schema syntax and no number an author picks or a differ must police.
- It does not add a second transport, a second router, or a per-language module. It is a codec
  mode inside the transports ADR 0017 kept.
- Its only ledger is `ContractLock`, which exists for signed-client compatibility already. A
  deployment without a lock still gets exact-match compaction per op.

What ADR 0017 objected to was the cost of a parallel surface competing with REST and RPC. C is
scoped to stay inside them.

## 7. Open questions

- **O1 — default.** Should generated clients opt in by default once the server supports it, or
  only behind a client option? Responses are never compact unless asked (§5.1), so a default-on
  client cannot break a server that lacks the mode. But it does make every CBOR body unreadable to
  `curl … | cbor2json` debugging.
- **O2 — framework-owned structs.** Should `RpcListInput`, `RpcGetInput` and the frame envelope
  get a fixed integer table, versioned by the RPC subprotocol (`cratestack.rpc.v1+cbor`)?
- **O3 — outbox and audit.** Should a model's `@@emit` payload be compact when its consumers are
  generated from the same schema? It saves the most where nothing compresses, but outlives its
  contract.
- **O4 — dictionary for streams.** Is option D worth revisiting for long `cbor-seq` and WS
  streams, where one dictionary amortizes over many items?
- **O5 — time and decimals.** After keys, the largest remaining bytes are `DateTime` as RFC 3339
  text (21–32 B) and `Decimal` as a string. Tag 1 or RFC 9581 tag 1001 for time is the next lever
  (`enum-ordinals.md` §6) and is independent of this design.

## 8. Measurements

RAN on 2026-11-06: `minicbor-serde` 0.7.1 (`std`), `serde` 1, `serde_json` 1, `uuid` 1 (`serde`),
`rust_decimal` 1 (`serde-str`, the workspace's feature), `chrono` 0.4 (`serde`), `flate2` 1 at its
default level, and `zstd` 0.13 at level 3. Ordinal enums are `u8` fields. Integer keys and
positional arrays are hand-written `Serialize` impls over the same struct (`serialize_map` with
`u8` keys, `serialize_tuple`). The dictionary is the CBOR encoding of the 30 identifiers the
probe's schema contains (300 B, raw content, not trained).

Shapes:

- `CreateProductInput { name, sku, description?, priceAmount: Decimal, currency, categoryId: Uuid,
  stockQuantity: Int, isActive, tags: [String] }`
- `Product` = the input plus `id: Uuid`, `createdAt`, `updatedAt: DateTime` and `version: Int`
- `TransferInput { sourceAccountId, destinationAccountId: Uuid, amount: Decimal, currency,
  reference? }`
- `Transaction { transactionId: Uuid, status: ordinal, sourceAccountId, destinationAccountId,
  amount, currency, fee, balanceAfter: Decimal, reference?, createdAt, settledAt?: DateTime }`
- 500 rows of `{ id: Uuid, eventType: ordinal, status: ordinal, createdAt: DateTime,
  amount: Decimal }`, the shape `enum-ordinals.md` §8 used

```text
minicbor-serde 0.7.1; gzip = flate2 default level; zstd level 3; dictionary = 300 B raw content
createProduct request    names=   205 gz=   207 zstd=   196 zstd+dict=   163 (+dcz    203) | intkeys=   133 gz=   156 | array=   124 | json=   264 json+gz=   229
createProduct response   names=   312 gz=   277 zstd=   272 zstd+dict=   226 (+dcz    266) | intkeys=   213 gz=   208 | array=   200 | json=   404 json+gz=   297
transfer request         names=   123 gz=   146 zstd=   127 zstd+dict=    87 (+dcz    127) | intkeys=    65 gz=    88 | array=    60 | json=   183 json+gz=   161
transfer response        names=   242 gz=   253 zstd=   236 zstd+dict=   164 (+dcz    204) | intkeys=   132 gz=   155 | array=   121 | json=   346 json+gz=   257
500-row list             names= 47465 gz= 15702 zstd= 15819 zstd+dict= 15761 (+dcz  15801) | intkeys= 31465 gz= 15321 | array= 28965 | json= 66463 json+gz= 18193
batch frame, op as text=100 B, op as integer=78 B (transfer input, integer keys)
```

The encoder, as in `enum-ordinals.md` §8, and the integer-key impl the probe generates per struct:

```rust
fn cbor<T: serde::Serialize + ?Sized>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    let mut s = minicbor_serde::Serializer::new(&mut out);
    s.serialize_unit_as_null(true); // as CborCodec::encode_into
    v.serialize(&mut s).unwrap();
    out
}

// For each struct, field i in the order listed:
impl Serialize for Keyed<'_, TransferInput> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(5))?;
        m.serialize_entry(&0u8, &self.0.source_account_id)?;
        m.serialize_entry(&1u8, &self.0.destination_account_id)?;
        m.serialize_entry(&2u8, &self.0.amount)?;
        m.serialize_entry(&3u8, &self.0.currency)?;
        m.serialize_entry(&4u8, &self.0.reference)?;
        m.end()
    }
}
```

## 9. If accepted: slices

1. **Core.** Derive the key table from an op contract (`cratestack-core/src/client_contract/`),
   with golden tests that a field reorder leaves the table unchanged and an added field yields a
   new digest with a new table.
2. **Codec.** The `serde` adapter in `cratestack-codec-cbor`, with tolerant decode (§5.3) and
   unknown-field drop (§5.4), and round-trip tests against derive-generated structs, including
   `Uuid`, `Decimal`, `Option` and nested types.
3. **Server.** Negotiation (§5.1), `ACCEPTED_CONTRACTS` tables from the lock, `ProjectedValue`,
   streams. REST and RPC in one PR, with batch frames still named.
4. **Clients.** Rust, Dart and TypeScript RPC, plus the swr, riverpod and fespalier layers, run
   through `just regen-examples`. Then an end-to-end test of an older generated client against a
   newer server whose lock accepts it, on Postgres.
5. **Signed transport.** The key-mode binding (§5.7), and an ADR 0006 amendment.
6. **Docs and skills.** `guides/payload-size.md` in cratestack-docs, and the payload/transport
   skill in cratestack-skills (`COVERAGE.md`).
