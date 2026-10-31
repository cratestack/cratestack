# step-up-procedures

Step-up authentication end to end, in Rust, with no database (cratestack#1245 story 5, on
cratestack#1244's `@authn`, ADR 0022).

```cstack
mutation procedure payInvoice(args: PayInvoiceArgs): Receipt
  @allow(auth() != null)
  @authn(maxAge: 300, amr: ["hwk"])

procedure balance(args: BalanceArgs): Balance
  @allow(auth() != null)
```

Paying needs a user who authenticated in the last five minutes with a hardware-bound key (RFC 8176
`hwk`). Reading the balance needs any signed-in user. A user who signed in twenty minutes ago with a
password reads the balance, and is refused the payment with RFC 9470's step-up signal before the
procedure runs. The generated client reads the refusal as `StepUpRequired`, the app re-proves, and
`retry_after_step_up` sends the call once more.

## What runs

| File | Role |
|---|---|
| `schema.cstack`, `schema-rpc.cstack` | The contract over REST and over `transport rpc`. They differ in the `transport rpc` line only. |
| `src/server.rs` | Both generated routers behind `SignedRequestAuthProvider`, CrateStack's in-house provider, with an id token verifier and no key registry. |
| `src/issuer.rs` | Mints the id tokens, which carry `auth_time` and `amr`, and publishes their key (JWKS). `Issuer::reprove` stands in for the re-authentication ceremony. |
| `src/device.rs` | The client's device key: it signs every request (`Authorization: Signature ..., id_jwt="..."`) through `RequestAuthorizer`. |
| `src/clients.rs` | The generated REST and RPC clients (`include_client_schema!`). |
| `src/flows.rs` | Call, re-prove, retry once, on REST, an RPC unary call and an RPC batch. |
| `src/main.rs` | The demo: the service on a loopback port, and the three flows against it. |

Where the facts come from: the in-house provider's product mapping copies the id token's `auth_time`,
and its `amr` plus `pop` (the request proved the bound key), onto the principal contract (ADR 0021
§2). The generated code compares them with `@authn` after `@allow` admitted the caller. Nothing in
`src/server.rs` checks freshness or methods.

What CrateStack does not do: run the re-authentication. That is the auth library's job
(cratestack#1244 leaves it out of scope). Here `Issuer::reprove` mints a token that says "`hwk`,
`user`, now". In a real deployment the auth service answers a fresh proof, such as a signature by the
device's user-present key or a passkey assertion.

## The refusal

On REST it is a `401` with the RFC 9470 challenge:

```text
WWW-Authenticate: Bearer error="insufficient_user_authentication", error_description="step-up authentication required", max_age="300"
{"code": "UNAUTHORIZED", "message": "step-up authentication required",
 "error": "insufficient_user_authentication",
 "details": {"max_age": 300, "amr": ["hwk"], "unmet": ["max_age", "amr"]}}
```

On RPC it is code `unauthenticated` with the same `error` member and `details`, as a unary answer or
as one frame of a batch. The other frames of the batch run.

## The client

```rust
let receipt = retry_after_step_up(
    move || async move { client.procedures().pay_invoice(args, &[]).await },
    |step_up| std::future::ready(session.reprove(&step_up)),
)
.await?;
```

`reprove` answers `false` when the user declines, and the first refusal comes back unchanged. When it
answers `true`, the call is sent once more and whatever it gives is returned, even a second refusal:
the helper never loops.

## Run

```bash
cargo run -p step-up-procedures-example
cargo test -p step-up-procedures-example
```

The tests run the real server on a loopback port and the real generated clients against it.
`tests/rest.rs` and `tests/rpc.rs` cover: one step-up then one payment, a fresh session that needs
none, the wire shape (401 and challenge, `unauthenticated`), a batch that refuses only the payment's
frame, a declined re-proof (no retry), and a re-proof that still falls short (one retry, then the
second refusal is returned).

The same flow on the generated Dart and TypeScript clients is in
[`flutter-riverpod`](../flutter-riverpod) and [`react-vite-swr`](../react-vite-swr).
