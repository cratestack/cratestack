# sync-notes

Offline-first sync between devices and a server, built on the **prototype**
`cratestack-sync` crate ([ADR 0020](https://cratestack.dev/internals/sync-engine-adr),
*Proposed*; design in [`docs/design/sync-engine.md`](../../docs/design/sync-engine.md)).

The page shows three devices: Alice's laptop, Alice's phone and Bob's laptop. Each one is a Dedicated
Worker running the same `schema.cstack` through `include_embedded_schema!`, compiled to wasm, with its
own SQLite database on OPFS. All three sync with one Postgres server that compiles that same schema
through `include_server_schema!`. A server panel below them shows the `notes` table, the change log
and the registered clients live.

## What it shows

| You do | What happens |
| --- | --- |
| Add or edit a note on any device | It is written to that device's SQLite immediately and queued as a named mutation (`note.create` / `note.update` / `note.delete`). The device pushes it, and the server replays it through the generated delegates under the schema's `@@allow` policies. The change log captures it, and the owner's other devices get a poke and pull it. |
| Flip a device to **Offline** | Writes keep working and show an **unsynced** badge. On reconnect the queue is pushed in order and the device rebases onto everything that happened meanwhile. |
| **Server-side write** (the PATCH form) | It goes through the generated REST route (`PATCH /api/notes/{id}`), not sync. A Postgres trigger captures it, so the owner's devices update anyway. |
| Add a title longer than 200 characters | The server rejects the mutation. The device rolls its optimistic write back (an undo log, not a refetch) and lists the mutation under *Rejected by server*. |
| Look at Bob's laptop | It never sees Alice's notes. Scopes come from the authenticated principal, and the read policy is re-checked when rows are pulled. |
| Reload the page | Every device comes back from OPFS before it touches the network. |

## Run it

Prerequisites: the `wasm32-unknown-unknown` target, a wasm-capable clang (see
[`embedded-browser-vite`](../embedded-browser-vite/README.md#prerequisites)), a `wasm-bindgen` CLI
matching the workspace lockfile (`build-web.sh` tells you the exact version), and a Postgres database.

```bash
# from the repository root
just pg-up                                   # or any Postgres you have
examples/sync-notes/build-web.sh             # wasm + JS glue into web/pkg/
DATABASE_URL=postgres://cratestack:cratestack@localhost:55432/cratestack_test \
  cargo run -p sync-notes-server             # http://localhost:8787
```

The server creates its table, the sync tables and the capture trigger on boot. `PORT` and `WEB_DIR`
override the defaults.

## Layout

```
sync-notes/
├── schema.cstack        # one schema, both roles
├── server/src/
│   ├── main.rs          # generated router + sync router + capture trigger + GC/poke background tasks
│   ├── sync.rs          # RowFetcher + MutationHandlers, written against the generated delegates
│   ├── auth.rs          # DEMO-ONLY header auth (x-demo-user) — replace before reusing anything here
│   └── demo.rs          # static files + the read-only /demo/server inspection endpoint
├── client/src/
│   ├── app.rs           # NotesApp: local mutators on the embedded delegates + the sync engine
│   ├── wasm.rs          # wasm-bindgen surface (JSON strings in/out)
│   └── tests_app.rs     # native tests of the same code
├── web/                 # index.html, app.js (UI), worker.js (one per device; owns all networking)
└── build-web.sh
```

## Read before copying

- **`auth.rs` is not authentication.** It trusts a header so the page can pose as Alice or Bob.
  Everything else is keyed on the context it returns, so this is the one file you must replace.
- **`/demo/server` is unauthenticated and lists every user's notes.** It exists so you can watch
  Postgres while you click. Do not mount it anywhere real.
- **"Offline" is simulated per device.** The worker stops making requests and drops its poke stream.
  To see real network loss, use DevTools' network throttling on the whole page.
- **The browser allows six HTTP/1.1 connections per host, and each device on this page holds one
  open for its poke stream.** In a real deployment each device is its own browser, and HTTP/2 lifts
  the limit anyway. Here, three devices plus the server panel is close to the ceiling, so don't add
  a fourth pane without moving the pokes to a shared connection.
- The local mutators in `client/src/app.rs` and the server handlers in `server/src/sync.rs` must
  agree on what each mutation name means. The proposed `@@sync` schema attribute would generate
  both from the schema. Until then, keeping them in step is your job.
