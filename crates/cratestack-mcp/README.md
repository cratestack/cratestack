# cratestack-mcp

**L4 — the MCP binding.** Serves a CrateStack schema's `@mcp(tool)` procedures to agents over the
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

## What a call goes through

1. The tool name is looked up. Unknown → JSON-RPC `-32602`.
2. The arguments are decoded into the procedure's `Args`. Failure → an `isError` result naming the field.
3. L3 admission (`cratestack-exec`), only when the application passed an `OpExecutor` with
   `StdioServer::with_executor`: rate limiting, then idempotency. An idempotency key travels in
   `_meta["dev.cratestack/idempotencyKey"]`; without one, nothing is reserved.
4. The procedure's generated `invoke_with_db`: `@allow`/`@deny`, delegated `@authorize(...)`, then the
   implementation, whose ORM calls carry `@@allow` in their SQL. `@computed` output fields are resolved
   by the same generated code REST uses.

Errors are `isError: true` results whose text is exactly REST's error envelope (`code`, `message`), so
MCP reveals nothing REST does not.

Not yet: Streamable HTTP (phase 4) and resources (phase 5). A schema that declares `@@mcp(resource: ...)`
does not compile until they ship.
