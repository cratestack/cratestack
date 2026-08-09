# react-vite-swr-client

Generated CrateStack TypeScript client package (`swr` preset — issues #304, #305). One file per
model under `src/models/`, holding that model's types and plain, framework-free `async` functions,
plus a sibling `<model>.hooks.ts` with one `useSWR`/`useSWRMutation` hook per operation. Import
exactly the functions/hooks you call — the plain-function file and its hooks file are separate
modules on purpose (see `src/swr/mod.rs`'s module doc): importing `<model>.ts` alone never requires
`swr`/React to be installed at all.

```ts
import { CratestackRuntime } from "react-vite-swr-client";
import { getBoard, listBoards } from "react-vite-swr-client/models/board";

const runtime = new CratestackRuntime("https://api.example.com", {
  basePath: "/api",
});
```

Every plain function takes the runtime as its first argument, so it's callable from a React
component, a server action, a plain Node script, or a test — importing one never pulls in React or
`swr` (see `tests/swr_runtime.rs` for the framework-free proof). The hooks are an optional layer on
top: install the peer dependencies (`swr`, `react`) only if you import a `.hooks` module.

## Models

- `src/models/board.ts` — `listBoards`, `getBoard`, `createBoard`, `updateBoard`, `deleteBoard`
- `src/models/board.hooks.ts` — `useBoards`, `useBoard`, `useCreateBoard`, `useUpdateBoard`, `useDeleteBoard`
- `src/models/task.ts` — `listTasks`, `getTask`, `createTask`, `updateTask`, `deleteTask`
- `src/models/task.hooks.ts` — `useTasks`, `useTask`, `useCreateTask`, `useUpdateTask`, `useDeleteTask`

```ts
const items = await listBoards(runtime, {
  query: { limit: 20, sort: ["-id"] },
});

const item = await getBoard(runtime, id);
const created = await createBoard(runtime, input);
const updated = await updateBoard(runtime, id, patch);
await deleteBoard(runtime, id);
```

The same operations as hooks, inside a component (note the `.hooks` subpath):

```tsx
import { useBoards, useBoard } from "react-vite-swr-client/models/board.hooks";

const { data: items } = useBoards(runtime);
const { data: item } = useBoard(runtime, id); // id may be null/undefined — skips the request
const { trigger: createBoard } = useCreateBoard(runtime);
const { trigger: updateBoard } = useUpdateBoard(runtime, id);
const { trigger: deleteBoard } = useDeleteBoard(runtime, id);
```

## Procedures

- `src/procedures.ts` — `estimateFocusMinutes`; `src/procedures.hooks.ts` —
`useEstimateFocusMinutesQuery`
```ts
import { estimateFocusMinutes } from "react-vite-swr-client/procedures";

const result = await estimateFocusMinutes(runtime, args);
```

## Hooks and cache invalidation

`src/swr-keys.ts` exports `swrKeys`, the single, shared key factory every hook builds its cache
key through — keys are nested under each model's/procedure's own schema-unique name (or route, for
REST), never derived through a lossy casing transform, so two similarly-named operations can never
collide (see that file's header comment).

Mutation hooks invalidate per a fixed rule, stated once per model's hooks file
(`src/models/*.hooks.ts`'s own header comment) and applied identically everywhere:

- **create** invalidates the model's list (every cached list, regardless of filter/pagination args).
- **update** invalidates the list **and** the mutated entity's own detail (both refetch).
- **delete** invalidates the list **and** drops the deleted entity's detail from the cache outright
  (no refetch — the entity is gone).

This rule is fixed, not configurable per call. Call `mutate`/`swrKeys` directly if you need
different invalidation. Procedure hooks never invalidate anything — that is model CRUD's job.

## Shared types

Enums and `type` blocks referenced by more than one model (or by no model at all — a declared but
unused type) live in `src/models/shared.ts` and are imported by their consumers. A type referenced
by exactly one model is defined inline in that model's own file instead. See
`cratestack-client-typescript`'s `src/swr/ownership.rs` for the computation that decides this.

## Decimal Fields

A `Decimal`-typed schema field is generated as a real `decimal.js`-backed `Decimal`
(cratestack#498, re-exported from `src/models/shared.ts`), not a `string` — construct one with
`new Decimal(input)`, format with `.toString()`, compare/do arithmetic with
`.plus()`/`.minus()`/`.cmp()`/`.equals()` instead of raw string/number operations. Like this
package's `default` preset, every generated function that decodes a server response (the per-model
`src/models/*.ts` functions and `src/procedures.ts`) calls a `reviveDecimalFields`/
`reviveDecimalScalar` decode hook, so a `Decimal`-typed field — including one reached through an
`include`d relation, or a procedure's own return type — is a real `Decimal` instance at runtime, not
just at the type level.