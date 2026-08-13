# react-vite-swr example

The epic #298 payoff: a real, running React app that consumes a `cratestack generate-typescript
--swr` client with **zero hand-written data-fetching code** — every `useSWR`/`useSWRMutation`
call, cache key, and fetcher in `web/src` comes from the generator, not from this app. (Issue #591
turned `--preset swr` into the additive `--swr` flag — `client/` below now carries the default
layout at `src/` *and* the swr layout at `src/swr/` in one package, reachable as
`react-vite-swr-client` and `react-vite-swr-client/swr` respectively; this example still only
imports from the `/swr` subpath, since that's the surface with zero hand-written data-fetching
code.)

## What it shows

- **A real Postgres-backed REST server** (`src/lib.rs`, `schema.cstack`): two models with a
  relation (`Task.board -> Board`) and one procedure (`estimateFocusMinutes`) — richer than a
  single-model fixture, without a `@@paged` model (the `swr` layout has a known gap there — see
  `docs`) and without a `to_camel_case` model-name collision (the default layout's own known gap).
- **A generated client, checked in** (`client/`) — run through the real CLI, not hand-written, and
  regenerable at any time (see below). Its own `README.md` documents the two-layer design
  (plain functions vs. hooks) and the fixed invalidation rule.
- **A React app** (`web/src`) that only imports generated code:
  - `useBoards`/`useCreateBoard` (`react-vite-swr-client/swr/models/board.hooks`) — list read + a
    mutation whose invalidation visibly refreshes that same list.
  - `useBoard`/`useTasks`/`useCreateTask`/`useUpdateTask`/`useDeleteTask`
    (`react-vite-swr-client/swr/models/task.hooks`) — a detail read, a list read, and the full CRUD
    mutation set, each invalidating per the generated hooks' fixed rule.
  - `useEstimateFocusMinutesQuery` (`react-vite-swr-client/swr/procedures.hooks`) — a query-kind
    procedure hook, conditionally fetched.
- **A plain-function script outside React** (`web/scripts/seed.ts`) — calls `listBoards`,
  `createBoard`, `createTask`, `estimateFocusMinutes` directly, no hook, no component, proving the
  two-layer design is real (see that file's own header comment for exactly how it's run and why).

## The full path: schema → generate → run

```bash
# 1. Bring up Postgres (repo root)
docker compose up -d postgres

# 2. Write/edit the schema
$EDITOR examples/react-vite-swr/schema.cstack

# 3. Generate the client (from the repo root) — this is what produced client/, and what
#    regenerates it after a schema change. `client/` is committed so a fresh clone runs without
#    a Rust toolchain; regenerate whenever schema.cstack changes.
cargo run -p cratestack-cli -- generate-typescript \
  --schema examples/react-vite-swr/schema.cstack \
  --out examples/react-vite-swr/client \
  --swr \
  --package-name react-vite-swr-client

# (Drift check instead of regenerating — used by CI, see below)
cargo run -p cratestack-cli -- generate-typescript \
  --schema examples/react-vite-swr/schema.cstack \
  --out examples/react-vite-swr/client \
  --swr \
  --package-name react-vite-swr-client \
  --check

# 4. Run the server
DATABASE_URL=postgres://cratestack:cratestack@localhost:55432/cratestack_test \
  cargo run -p react-vite-swr-example
# -> react-vite-swr-server listening on http://127.0.0.1:3210 (routes under /api)

# 5. Install + run the app (separate shell, from this directory)
cd examples/react-vite-swr
pnpm install
pnpm --filter react-vite-swr-web run dev
# -> http://localhost:5173

# 6. Optional: seed demo data + call a plain function outside React
cd web && pnpm run seed
```

Open `http://localhost:5173`: the Boards screen lists boards (`useBoards`) and lets you add one
(`useCreateBoard`) — watch the list update with no page reload and no manual refetch call anywhere
in `BoardsScreen.tsx`. Click a board for its detail screen: tasks (`useTasks`), add/toggle/delete
(`useCreateTask`/`useUpdateTask`/`useDeleteTask`), and a focus-time estimate computed by the
`estimateFocusMinutes` procedure hook, which recomputes automatically as the task list changes.

## Layout

| Path | What |
|---|---|
| [`schema.cstack`](schema.cstack) | `Board`/`Task` models (one relation) + `estimateFocusMinutes` procedure |
| [`src/lib.rs`](src/lib.rs) | `include_server_schema!`, header auth, the procedure impl, `build_router`/`ensure_schema` |
| [`src/main.rs`](src/main.rs) | Server entry point — binds `127.0.0.1:3210`, routes under `/api` |
| [`tests/smoke.rs`](tests/smoke.rs) | Offline router-build test + a real-Postgres CRUD/procedure round trip (`CRATESTACK_TEST_DATABASE_URL`) |
| [`client/`](client) | Generated `--swr` package (checked in) |
| [`web/`](web) | The React + Vite app — `src/screens/`, `src/runtime.ts`, `scripts/seed.ts` |

## Run the Rust tests

```bash
cargo test -p react-vite-swr-example                                  # offline test only
CRATESTACK_TEST_DATABASE_URL=postgres://cratestack:cratestack@localhost:55432/cratestack_test \
  cargo test -p react-vite-swr-example                                # + real Postgres round trip
```

## Scope / known gaps carried in from the swr layout

- The `swr` layout doesn't support `@@paged` models yet — this schema doesn't use one, on purpose.
- `list()`'s server-side filtering (`where`/structured filters) isn't wired into the generated
  functions yet — `BoardDetailScreen` reads the full task list and filters by `boardId`
  client-side rather than relying on that.
- No login screen — a static `x-auth-id: 1` header stands in for real auth (`src/runtime.ts`),
  matching every other example in this repo.

## Ergonomic findings from building this (see the PR description for the full writeup)

- The sibling-hooks-file subpath-import pattern (`.../swr/models/board.hooks`) works as designed
  once `package.json` actually declares the subpath in `exports` — it didn't (fixed as part of
  the PR that introduced it, and re-nested under `/swr` when `--preset swr` became `--swr`).
- `useUpdateTask(runtime, id)`/`useDeleteTask(runtime, id)` bind `id` at hook-call time, not at
  `trigger()` time — see `web/src/screens/TaskRow.tsx`'s header comment for the one-component-per-
  row pattern this requires in a list.
- Every hook takes `runtime` as an explicit first argument (no context provider) — fine for an app
  this size; worth a second look for a larger app calling dozens of hooks per screen.
