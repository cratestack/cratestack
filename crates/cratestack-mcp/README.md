# cratestack-mcp

**L4 — the MCP binding.** Serves a CrateStack schema's `@mcp(tool)` procedures and `@@mcp(resource)`
models to agents over the
[Model Context Protocol](https://modelcontextprotocol.io), revision `2026-07-28`, through the same
generated policy check as REST and RPC (ADR 0002, cratestack#1033).

You do not depend on this crate directly. Enable the `mcp` feature on the facade your schema already
uses — `cratestack-pg` or `cratestack-api` (`cratestack-sqlite` and `cratestack-client` have none: the
embedded role enforces no policy, and the client facade serves nothing):

```toml
cratestack = { package = "cratestack-pg", version = "0.12", features = ["mcp"] }
```

```cstack
mcp {
  expose = [tools]
}

mutation procedure publishPost(args: PublishPostInput): Post
  @allow(auth().role == "admin")
  @mcp(tool: "publish_post", description: "Publish a draft post.")
```

The schema macro then generates `cratestack_schema::mcp`, whose `tools(db, registry, resolvers)`
value is the tool table. Serve it over stdio with an explicit caller identity — there is no default:

```text
let ctx = cratestack::SystemContext::for_service("support-agent").into_context();
cratestack::mcp::StdioServer::new(cratestack_schema::mcp::tools(db, registry, resolvers), ctx)?
    .serve()
    .await?;
```

Send `tracing` to stderr (`tracing_subscriber::fmt().with_writer(std::io::stderr)`): stdout carries
only MCP messages. The server exits when stdin closes.

## Streamable HTTP

Or mount it on your axum router, authenticated by the `AuthProvider` your REST routes use
(cratestack#1039). The allowed browser origins and the provider are required, and an empty origins
list is refused:

```text
let resource = ProtectedResource::new("https://api.example.com/mcp", ["https://auth.example.com"]);
let mcp = StreamableHttpServer::builder(tools, auth_provider, ["https://app.example.com"], resource)
    .build()?;
let app = Router::new()
    .nest_service("/mcp", mcp.service())
    .merge(mcp.metadata_router()); // RFC 9728 metadata, always at the root
```

A foreign `Origin` gets 403 and `GET`/`DELETE` get 405. A missing or rejected token gets 401 with
`WWW-Authenticate: Bearer resource_metadata="…"`. A token in the query string (`?access_token=`) gets
400 `invalid_request` before your provider runs, and a mirrored MCP header sent twice gets 400 /
`-32020`. The token is removed from the request before `rmcp`
sees it, and every call runs under the `CratestackContext` your provider built, through the same
admission and policy as stdio. **Your provider must check the token's audience** against the resource
identifier: MCP requires it, and CrateStack ships no generic OAuth provider in v1 (ADR 0002 Q5).
`tests/support/token.rs` is an example.

## What a call goes through

1. The tool name is looked up. Unknown → JSON-RPC `-32602`.
2. The arguments are decoded into the procedure's `Args`. Failure → an `isError` result naming the field.
3. L3 admission (`cratestack-exec`), only when the application passed an `OpExecutor` with
   `with_executor` (on either transport): rate limiting, then idempotency. An idempotency key travels in
   `_meta["dev.cratestack/idempotencyKey"]`; without one, nothing is reserved. A rate-limit store
   lookup is bounded at 500ms (`DEFAULT_STORE_TIMEOUT`), and a failing store follows the
   `StoreErrorPolicy` passed to `StdioServer::with_store_error_policy` — the same type
   `cratestack_axum::ratelimit::RateLimitLayer` takes, with the same default (serve through an
   unreachable store, refuse any other failure). Pass `StoreErrorPolicy::Deny` if you chose it on HTTP.
4. The procedure's generated `invoke_with_db`: `@allow`/`@deny`, delegated `@authorize(...)`, then the
   implementation, whose ORM calls carry `@@allow` in their SQL. `@computed` output fields are resolved
   by the same generated code REST uses.

Errors are `isError: true` results whose text is exactly REST's error envelope (`code`, `message`), so
MCP reveals nothing REST does not.

## Resources (`cratestack-pg` only)

A model annotated `@@mcp(resource: "posts")` is a read-only resource (cratestack#1040):

```cstack
mcp {
  name = "blog"
  expose = [resources]
}
```

- `cratestack://blog/posts/{id}` reads one record, shaped exactly like REST's `GET /posts/{id}`
  (same serializer, so `@server_only` fields are absent and `@computed` fields resolved).
- `cratestack://blog/posts{?limit,cursor}` reads a page, `{"items": [...], "nextCursor": "..."}`, in
  primary-key order. `limit` defaults to 50 and is clamped (not refused) at 200, or at the model's
  `max_page_size:` when lower. Pass `nextCursor` back as `cursor`; a cursor this server did not issue for
  that resource is `-32602`.

`blog` is the block's `name`, required whenever `expose` lists `resources` and refused otherwise: a quoted
string of lowercase letters, digits and `-`. It is stated rather than taken from the file's name so that
renaming the `.cstack` file never moves a URI an agent holds.

Reads go through the same ORM calls REST's handlers make, under the caller's context (the one passed to
`StdioServer::new`, or the one your `AuthProvider` built for this HTTP request), so `@@allow("read", ...)` is in the SQL: a row the caller may not read and a
row that does not exist are the same `-32602` "resource not found". Reads pass the same rate-limit
admission as tool calls, in the caller's bucket. Every result is `cacheScope: private`, `ttlMs: 0`.
