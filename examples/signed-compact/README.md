# signed-compact

A COSE-sealed RPC service ([ADR 0006](https://cratestack.dev/internals/cose-envelope-adr)) that also speaks **compact wire keys**
([`docs/design/compact-wire-keys.md`](../../docs/design/compact-wire-keys.md)) inside the seal,
with its generated Rust client. Procedures-only (`transport rpc`, `db = None`); no database.

Two crates, because one crate cannot rename two facades to `cratestack`:

| Crate | Facade | Role |
|---|---|---|
| [`server/`](server) | `cratestack-api`, feature `cose` | `include_server_schema!` + `EnvelopeLayer` (Mac0, `Required`, audience `payments`) |
| [`client/`](client) | `cratestack-client`, feature `cose` | `include_client_schema!` + `with_envelope(..)` + `Client::with_compact_keys(..)` |

Both include the shared [`schema.cstack`](schema.cstack): `transferFunds(input: TransferInput): Transaction`.
REST works the same way (`compact_client_sealed_rest.rs` in `cratestack-api` is the twin of the RPC test);
this example uses RPC.

## What it shows

- The server wraps the generated router in the envelope layer built by the generated
  `envelope_layer(..)` helper, then opts the compact payload types in with
  `.payload_media_types(["application/cbor", COMPACT_CBOR], ["application/cbor", COMPACT_CBOR])`.
- The client signs with `with_envelope` and opts into compact keys with `with_compact_keys`.
  It is adaptive: the first request goes out named, the answer comes back compact, and from then on
  that op is sent compact.

## On the wire

The outer `Content-Type` is `application/cose...` in every row; the inner payload type
(`Cratestack-Payload-Type`) is what changes:

| | Request, inner payload type | Response, inner payload type |
|---|---|---|
| Call 1 | none (plain CBOR, named keys) | `application/vnd.cratestack.compact+cbor` |
| Call 2 | `application/vnd.cratestack.compact+cbor` | `application/vnd.cratestack.compact+cbor` |

The handlers always see named keys: the generated router's compact layer rewrites the opened body
back before dispatch.

### Why there is no `+cose` payload type

The outer type names the envelope (COSE_Sign1 or COSE_Mac0); the key mode belongs to the payload
inside it. Only the inner payload type is in the signature's AAD, so it is the one place the mode is
authenticated: an outer `+cose` spelling would sit in an unsigned header and say the same thing twice.
JSON under a seal travels the same way (outer `application/cose`, inner `application/json`). See
section 5.1 ("Sealed (COSE)") of the design doc and its "Why not `+cose`" bullet.

### The opt-in has to be both directions

`payload_media_types(request, response)` takes two lists, and compact must be in both.

- **Response only:** the client learns compact exists and seals its next request compact; the
  server refuses that type with an **unsigned `415`**. The client turns that op back to named for the
  next call. The failed call is returned, not retried.
- **Neither (an old server):** the answer is plain, the client stays named, and nothing ever fails.
  `tests/sealed_compact.rs` covers this case.

## Run

```bash
cargo run -p signed-compact-server    # 127.0.0.1:3000, PORT overrides
cargo run -p signed-compact-client    # REMOTE_URL overrides
```

The server prints the payload type of each call it served. The client cannot observe the
inner type itself, so it states what the protocol does; the server log and the test confirm it.

`SIGNED_COMPACT_SECRET` (32 bytes) overrides the HMAC secret on both sides. The built-in
demo secret is published in this repository and is **for local use only**.

## Tests

```bash
cargo test -p signed-compact-client
```

`client/tests/sealed_compact.rs` serves `signed_compact_server::app(..)` in-process on an ephemeral
port and asserts the table above (values round-trip, payload types, outer `application/cose`), plus a
server that does not opt in, where the client stays named.

## Further reading

- [Signed transport guide](https://cratestack.dev/guides/signed-transport)
- [Payload size guide](https://cratestack.dev/guides/payload-size)
