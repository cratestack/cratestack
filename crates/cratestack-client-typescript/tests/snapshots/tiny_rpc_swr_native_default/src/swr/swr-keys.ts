// Shared cache-key factory for the `swr` preset's hooks (issue #305).
// Every `useSWR`/`useSWRMutation` hook in this package builds its key
// exclusively through `swrKeys` below, never by hand-writing a key
// literal in a hook body — see `src/models/*.ts`/`src/procedures.ts`'s
// own header comments for the invalidation rule these keys feed.
//
// Collision-freedom: keys are nested under each model's own literal
// schema name (`swrKeys.model.User`, ...) and each procedure's own
// literal name (`swrKeys.procedure.someProcedure`) — the same
// identifiers RPC dispatch already routes "model.<Name>.<verb>" /
// "procedure.<name>" through, so `cratestack-parser`'s uniqueness
// checks already guarantee no two entries can collide. This is a
// deliberately different derivation from `crate::views::ModelApiView`'s
// `*_query_key` fields (`{model}List` etc., built for the
// react-query preset's flat *named* key object): those are derived
// through `to_camel_case` (e.g. `list_query_key` = "<model>List"),
// which can collapse two differently-named models to the same property
// name (see
// `cratestack-client-typescript`'s swr_generator.rs test fixture
// `swr_key_collision.cstack` for a real example) — nesting under the
// raw, parser-unique name sidesteps that risk entirely rather than
// working around it.
//
// `get`/query-procedure keys return `null` when their argument is
// nullish, which is SWR's documented conditional-fetching idiom — see
// https://swr.vercel.app/docs/conditional-fetching — a `useSWR(null,
// ...)` never fires a request, so a hook can be called before its id/
// args are known without firing a request with `undefined`.
//
// `<Model>.listMatches` is a key *filter* (not a key) for `mutate()`'s
// filter-function overload: it matches every cached list key for this
// model regardless of its `query` argument, so invalidating "the list"
// after a mutation doesn't require knowing which filtered/paginated
// variants happen to be in cache.

import type { CratestackRpcListQuery } from "./queries.js";
import type { EchoNameArgs } from "./procedures.js";

export const swrKeys = {
  model: {
    Widget: {
      list: (query: CratestackRpcListQuery = {}) => ["model.Widget.list", query] as const,
      get: (id: number | null | undefined) =>
        id == null ? null : (["model.Widget.get", id] as const),
      listMatches: (key: unknown): boolean =>
        Array.isArray(key) && key[0] === "model.Widget.list",
      create: () => ["model.Widget.create"] as const,
      update: (id: number) => ["model.Widget.update", id] as const,
      delete: (id: number) => ["model.Widget.delete", id] as const,
    },
  },
  procedure: {
    echoName: (args: EchoNameArgs | null | undefined) =>
      args == null ? null : (["procedure.echoName", args] as const),
  },
} as const;