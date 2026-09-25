# mcp-operator

CrateStack's MCP operator end to end (ADR 0002, cratestack#1041). One Postgres-backed schema serves
two procedures as MCP tools and one model as a read-only MCP resource, to agents, over **stdio** and
over **Streamable HTTP**. Both run through the same generated policy checks as REST and RPC. The HTTP
side sits behind an example audience-checking `AuthProvider`.

`just mcp-conformance` drives both transports with a real third-party client, the official
[MCP Inspector](https://github.com/modelcontextprotocol/inspector) CLI, at protocol `2026-07-28`
(see [Conformance](#conformance)).

## The schema

[`schema.cstack`](schema.cstack):

```cstack
mcp {
  name = "blog"
  expose = [tools, resources]
}

auth Caller {
  id String
  role String
}

model Post {
  id Int @id
  authorId String
  title String
  published Boolean

  @@allow("read", published || authorId == auth().id)
  @@allow("update", auth() != null && auth().role == "editor")
  @@mcp(resource: "posts")
}

procedure recentPosts(limit: Int): Post[]
  @allow(auth() != null)
  @mcp(tool: "recent_posts", description: "The newest posts you may read, newest first.")

mutation procedure publishPost(id: Int): Post
  @allow(auth() != null && auth().role == "editor")
  @mcp(tool: "publish_post", description: "Publish a draft post. Editors only.")
```

What an agent sees:

| Surface | What it is | Who gets an answer |
|---|---|---|
| `recent_posts` tool | Query procedure, `readOnlyHint: true` | Any signed-in caller. The rows are filtered by `@@allow("read", ...)` in the SQL. |
| `publish_post` tool | Mutation procedure, `readOnlyHint: false`, `idempotentHint: false` | Editors only. Anyone else gets an `isError` result whose text is REST's error envelope, `{"code":"FORBIDDEN","message":"procedure policy denied this operation",...}`. The implementation never runs. |
| `cratestack://blog/posts/{id}` | One post, read-only | A post the caller may not read answers exactly like one that doesn't exist: `-32602` "resource not found". |
| `cratestack://blog/posts{?limit,cursor}` | A page of posts, in id order | Only rows the caller may read. The page size defaults to 50 and is capped at 200. |

The server creates a `posts` table and seeds four rows on start, if they are missing:

| id | author | published | readable by |
|---|---|---|---|
| 1 | `u-1` | yes | anyone signed in |
| 2 | `u-1` | no | `u-1` |
| 3 | `u-2` | no | `u-2` |
| 4 | `u-2` | yes | anyone signed in |

The caller's `id` and `role` come from a token (see [Tokens](#tokens-and-the-example-authprovider)).
A member `u-1` sees posts 1, 2 and 4, and never sees post 3.

## Why this is its own workspace

This crate turns on `cratestack-pg`'s `mcp` feature. As a root workspace member it would switch that
feature on for every sibling in `cargo test --workspace`, because Cargo unifies features across the
members it builds. `cratestack-macros`' `tests/ui_mcp.rs` pins the feature-*off* refusal, so it would
start failing. So the crate is listed in the root `Cargo.toml`'s `[workspace] exclude` and has its
own `Cargo.lock`. Run its commands from this directory, not with `-p`.

## Prerequisites

- **Postgres.** `DATABASE_URL`, defaulting to the repository's compose database
  (`postgres://cratestack:cratestack@localhost:55432/cratestack_test`; `just pg-up` starts it).
- **A signing key**, at least 32 bytes, in `MCP_EXAMPLE_SIGNING_KEY`. There is no built-in default:
  a key compiled into the binary would let anyone mint an editor's token.

```bash
cd examples/mcp-operator
just pg-up                                  # or point DATABASE_URL at your own Postgres
export MCP_EXAMPLE_SIGNING_KEY="$(openssl rand -hex 32)"
cargo build
```

## Run over stdio

Over stdio, the MCP spec says credentials come from the environment. The server verifies the token in
`MCP_EXAMPLE_TOKEN` and serves every request as that caller. Without a valid token it doesn't start,
because there is no default identity (ADR 0002 Q1). A stdio token's audience is `cratestack://blog`.

```bash
export MCP_EXAMPLE_TOKEN="$(cargo run -q -- mint-token --audience cratestack://blog --id u-1 --role editor)"
cargo run -q -- stdio                       # newline-delimited JSON-RPC on stdin/stdout; logs on stderr
```

To point an MCP client at it, give the client the command and the environment. The MCP Inspector
CLI:

```bash
npx @modelcontextprotocol/inspector@2.8.0 --cli ./target/debug/mcp-operator-example stdio -- \
  --protocol-era modern \
  -e DATABASE_URL="$DATABASE_URL" \
  -e MCP_EXAMPLE_SIGNING_KEY="$MCP_EXAMPLE_SIGNING_KEY" \
  -e MCP_EXAMPLE_TOKEN="$MCP_EXAMPLE_TOKEN" \
  --method tools/call --tool-name publish_post --tool-args-json '{"id":2}'
```

For a desktop client that reads the common `mcpServers` configuration:

```json
{
  "mcpServers": {
    "blog": {
      "command": "/absolute/path/to/examples/mcp-operator/target/debug/mcp-operator-example",
      "args": ["stdio"],
      "env": {
        "DATABASE_URL": "postgres://cratestack:cratestack@localhost:55432/cratestack_test",
        "MCP_EXAMPLE_SIGNING_KEY": "<your key>",
        "MCP_EXAMPLE_TOKEN": "<a token for cratestack://blog>"
      }
    }
  }
}
```

The client must speak protocol `2026-07-28`. The server offers only that revision, so a client that
opens with a legacy `initialize` is refused with `-32022 Unsupported protocol version` instead of
being negotiated down. With the Inspector, that is `--protocol-era modern`.

## Run over Streamable HTTP

```bash
cargo run -q -- http                        # http://127.0.0.1:8787/mcp
# options: --addr 127.0.0.1:8787  --resource <public URL of /mcp>  --allowed-origin <origin> (repeatable)
```

The endpoint's URL is its **resource identifier**. A token must name exactly that URL as its
audience, and the RFC 9728 metadata document at `/.well-known/oauth-protected-resource` names it too.
A token for any other audience, including a stdio token, gets `401`:

```bash
TOKEN="$(cargo run -q -- mint-token --audience http://127.0.0.1:8787/mcp --id u-1 --role member)"

npx @modelcontextprotocol/inspector@2.8.0 --cli http://127.0.0.1:8787/mcp -- \
  --transport http --protocol-era modern \
  --header "Authorization: Bearer $TOKEN" \
  --method resources/read --uri cratestack://blog/posts/1
```

For a client that takes a URL and headers:

```json
{
  "mcpServers": {
    "blog": {
      "type": "http",
      "url": "http://127.0.0.1:8787/mcp",
      "headers": { "Authorization": "Bearer <a token for http://127.0.0.1:8787/mcp>" }
    }
  }
}
```

What the endpoint enforces, from `cratestack-mcp`: a foreign browser `Origin` gets `403` (the allowed
list defaults to the Inspector web UI's `http://localhost:6274`; pass `--allowed-origin` to change
it, and an empty list is refused). `GET` and `DELETE` get `405`. A missing or refused token gets
`401` with `WWW-Authenticate: Bearer resource_metadata="…"`. A token in the query string gets `400`.
The token is removed from the request before the MCP handler sees it, and it is never passed on to
another service.

## Tokens and the example `AuthProvider`

[`src/token.rs`](src/token.rs) is **example code, not a published API**. CrateStack v1 ships no
generic OAuth access-token provider (ADR 0002 Q5): your application brings its own, verifying its
authorization server's JWTs against that server's JWKS. This stand-in uses a compact
`<claims>.<signature>` token (base64url JSON claims, HMAC-SHA256), so the example needs no
authorization server, and `mint-token` plays the authorization server's part.

What carries over to a real provider is the order of the checks: signature, issuer, **audience**,
expiry, and only then a context. The audience check is the one MCP requires. Without it, a token a
user got for any other service could be replayed here. Only `id` and `role` reach the
`CratestackContext`, named one by one: copying every claim across would let whoever mints tokens set
any `auth()` field a policy reads. `tests/token.rs` refuses one broken property per test.

## Conformance

```bash
just mcp-conformance                        # from the repository root
```

The recipe installs `@modelcontextprotocol/inspector@2.8.0` (with `--ignore-scripts`, into a
throwaway directory) and builds this example. It starts a throwaway `postgres:18-alpine` container,
unless `MCP_CONFORMANCE_DATABASE_URL` names a database. Then
[`conformance/run.mjs`](conformance/run.mjs) drives the Inspector CLI through these cases, over
stdio and over Streamable HTTP:

- discovery (`server/discover` negotiates `2026-07-28`);
- `tools/list`;
- a successful `tools/call`;
- a policy-denied `tools/call` (`isError`, `FORBIDDEN`, from the procedure's `@allow`), then proof
  that it never ran;
- `resources/list`;
- `resources/read` for a record, for a collection, and for a hidden row, which answers exactly like a
  missing one.

It also runs the refusals: no token, a token for another audience, and a legacy-era client. It fails
unless every expected case ran and passed. CI runs it in the `mcp-example` job.

## Tests

```bash
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"   # rootless Docker
CRATESTACK_REQUIRE_DB=1 cargo test -- --test-threads=1
```

`tests/token.rs` needs no database. `tests/serve.rs` starts a Postgres testcontainer. It skips, and
still prints `ok`, when Docker is unreachable, unless `CRATESTACK_REQUIRE_DB` is set. Read
`finished in` to tell a skip (`0.00s`) from a run (seconds).
