# Prepared idempotent calls

A compact-capable client may learn to send compact bodies after an earlier
response. Idempotency checks compare the request that arrived on the wire, so
re-encoding a retry under the same key can cause a conflict. In-memory key pins
preserve an encoding for one client lifetime. Their bounded storage does not
survive a restart; once full, new keys remain named for that instance.

For retries across restarts, prepare the unary mutation and persist its snapshot
in the application's own durable journal **before the first send**. Restore that
snapshot and execute it for every retry. Preparing makes no request and the
runtime does not provide a queue or journal. Protect snapshots as sensitive
business payloads. The same key with changed input is still a conflict.

- Rust: call `prepare_call`, persist `PreparedCall::to_bytes`, restore with
  `PreparedCall::from_bytes`, then call `execute_prepared` (or its typed variant).
  Enable compact key tables first and supply the route and idempotency header.
- TypeScript: call the REST or RPC runtime's `prepare`, persist the JSON value,
  and pass the restored value to `executePrepared`. RPC custom unary links are
  unsupported for prepared calls and rejected during preparation.
- Dart: call the HTTP adapter's `prepare` or `prepareCall`, persist `toJson`,
  restore with `CratestackPreparedCall.fromJson`, and call `executePrepared`.
  If the generated client uses a custom REST base path, configure the same
  `preparedBasePath` on its adapter for route validation.

Snapshots freeze the inner bytes, media type, route, key, conditional write
header, and contract information. Only `Idempotency-Key` and `If-Match` are
persisted as application headers; transport media and selector headers are
recorded separately where needed. Authentication headers are not stored and URL userinfo is rejected. Query
strings and bodies are persisted and may themselves contain secrets.
Supply fresh credentials through the Rust authorizer, TypeScript default
headers, or Dart's `authorization` argument or transport on every attempt.
Rust COSE calls create a new seal and verify each response against that attempt;
a sealed envelope is never stored in the snapshot.

Restoration is deliberately not a schema migration: a changed service, codec,
route, key table or operation contract is rejected before network I/O. Keep the
matching client version available until its journal is drained. Snapshot formats
are bounded and versioned; applications should handle invalid or incompatible
snapshots as errors rather than reconstructing a request under the old key.

CBOR snapshots are checked as one well-formed item before sending, including
UTF-8 text, complete container pairs, and a nesting limit of 256. The check
accepts non-preferred encodings and leaves the stored bytes unchanged. It does
not evaluate tag semantics or reject duplicate map keys; duplicate handling
follows the receiving codec. Protect the caller-owned journal against tampering.

For ordinary Rust compact-capable calls, pass `Idempotency-Key` in the per-call
headers. A key added only by a request authorizer is rejected before network
I/O, because compact encoding has already been chosen when that authorizer
runs. Headers added or changed later by a custom HTTP client's defaults or middleware
are outside the runtime's control. Such transports must preserve the frozen
identity headers and their absence, for ordinary and prepared calls alike.
