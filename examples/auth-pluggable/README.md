# auth-pluggable

One `.cstack` schema, one test suite, three authentication providers (cratestack#1245). It proves the
claim of ADR 0021 (cratestack-docs `internals/auth-seam-adr.md`): CrateStack's own authentication and
an external library plug into a CrateStack server through the same public `AuthProvider` seam, with
the same behaviour on every transport, and CrateStack depends on neither.

| Binary | Provider | File |
|---|---|---|
| `inhouse` | `SignedRequestAuthProvider` (`cratestack-auth`) then a COSE-verified signer (`cratestack-cose`), as one first-success chain | [`src/providers/inhouse.rs`](src/providers/inhouse.rs) |
| `authkestra-devsig` | `authkestra_devsig::verify`: a device-signed `X-Signature` per request, plus an issuer-signed `X-Attestation` binding the device key | [`src/providers/devsig.rs`](src/providers/devsig.rs) |
| `authkestra-oidc` | `authkestra-resource`'s `JwtStrategy`: an access token from `authkestra-op`, bound to the client's key with DPoP (RFC 9449) | [`src/providers/oidc.rs`](src/providers/oidc.rs) |

Everything else is shared: [`schema.cstack`](schema.cstack), the procedures in
[`src/lib.rs`](src/lib.rs), and the test suite in [`tests/support/mod.rs`](tests/support/mod.rs).
The binaries read their keys from the environment (see the top of each `src/bin/*.rs`) and listen on
`127.0.0.1:8080`.

## What the suite checks

Each provider's test (`tests/inhouse.rs`, `tests/devsig.rs`, `tests/oidc.rs`) says how to sign a
request as an ordinary user and as an admin, and names one credential the provider refuses. Then
`suite()` runs:

1. **The conformance kit** (`cratestack-auth-conformance`): the call succeeds as the same subject on
   REST, RPC unary and RPC batch (each at the root and under a mount prefix) and on MCP; the refusal
   reads the same everywhere (status, code, message, members, challenges); the provider is handed
   the canonical request exactly; it is called once per request.
2. **The example's own policies**: an anonymous `whoAmI` is refused; `whoAmI` returns the user's
   subject; `adminReport` (`@allow(hasRole("admin"))`) refuses the user with `403` and serves the
   admin.

The schema and the suite know nothing of the provider. The procedures read the caller through the
principal contract alone (`ctx.subject()`, `ctx.roles()`), which every provider fills:

| | in-house | device signatures | OIDC + DPoP |
|---|---|---|---|
| `sub` | `svc:<keyId>`, or `cose:<thumbprint>` | the attestation's `sub` | the token's `sub` |
| `roles` | `admin` for configured keys, else `caller` | `att.roles` | the identity's `roles` attribute |
| `cnf.jkt` | — (service keys carry no key binding) | the device key's thumbprint | the DPoP key's thumbprint |
| `amr`, `acr`, `auth_time` | — | — | the identity's attributes |
| a refusal | `401`, `signature verification failed` | `401`, `{"error": "<devsig code>"}` | `401`, `{"error": "invalid_token"}` and `WWW-Authenticate: DPoP error="invalid_token"` |

## Run it

```bash
cargo test --locked                       # the three providers through one suite
../../.ci/auth-pluggable-check.sh         # no CrateStack crate in this graph reaches authkestra*
```

CI's `auth-pluggable` job runs both, plus `cargo fmt --check` and `cargo clippy -D warnings`.

## Why it is its own workspace

It is the only crate in this repository that depends on Authkestra. Outside the root workspace,
Authkestra never enters the root `Cargo.lock`, which `.ci/auth-independence-check.sh` (ADR 0021 §7)
refuses to see it in. Inside this example, `.ci/auth-pluggable-check.sh` runs `cargo tree` on every
CrateStack crate in the graph and fails if one of them reaches an `authkestra*` package; it also
checks that the example itself does, so the check can fail. Authkestra is pinned to one commit
(`9d40169`) in `Cargo.toml` and `Cargo.lock`: a bump is a deliberate change.

## Line counts, and why two providers are longer than ~50

The epic set "under ~50 lines each" as a signal, not a gate. `inhouse.rs` is 45 lines. The two
Authkestra providers are longer because each does work its library leaves to the integrator:

- **`devsig.rs` (71 lines)** builds `authkestra_devsig::SignedRequest` from the `RequestContext` and
  maps `VerifyError::code()` onto an `AuthRefusal` member. Authkestra's own axum adapter answers a
  refusal with a plain-text body; a device-signature client sorts refusals by a JSON `error` code
  (renew on `attestation_expired`, sign out on `key_not_bound`), which the provider sets.
- **`oidc.rs` (81 lines)** rebuilds `http::request::Parts` from the `RequestContext` (what
  `JwtStrategy::authenticate` takes) and maps the token onto the principal contract.

## Findings for Authkestra (cratestack#1245)

None of these needed anything private in CrateStack; they are what the providers work around.

- **`JwtStrategy` refuses with no reason.** An invalid token or proof is `Ok(None)`; the reason goes
  to `tracing` only. The provider therefore answers every refusal with one code, `invalid_token`,
  and cannot tell a client "your proof is stale" from "your token is expired".
- **No `use_dpop_nonce`.** Neither `authkestra-op` nor `authkestra-resource` issues a DPoP nonce.
- **The JWKS can only be fetched over HTTP.** `JwksCache` has no in-memory source, so the OIDC tests
  serve the OP's JWKS on `127.0.0.1`.
- **`roles`, `amr`, `acr` and `auth_time` are not access-token claims.** `authkestra-op` puts `amr`,
  `acr` and `auth_time` in the ID token only; the access token embeds the identity, whose string
  attributes the provider reads instead.
- **No device-signature profile or conformance vectors yet.** Authkestra's device-signature scheme
  and fespalier's client (`fespalier_sign_keypair`) differ in their refusal bodies, codes and
  attestation shape; the planned "profile v1" with shared vectors does not exist. The device fixture
  (`tests/devsig.rs`) mints exactly what `authkestra-devsig` verifies at the pinned commit.

## DPoP and the canonical request

A provider is handed the declared route without the router's mount prefix (ADR 0021 §3), and
`JwtStrategy` checks a proof's `htu` against `dpop_resource_origin` plus that path. A client therefore
signs `htu = <origin><declared route>`. A service mounted under a prefix puts the prefix in the origin
it configures (`https://api.example.com/v1`), so a client signing the URL it actually sends signs the
same bytes.
