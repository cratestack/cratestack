// Generated SWR hooks for this model (issue #305) — a sibling file to
// `./board` (the plain, framework-free functions from issue
// #304), not appended into it: see `cratestack-client-typescript`'s
// `src/swr/mod.rs` module doc for why hooks and plain functions can't
// share a file without breaking the framework-free guarantee (`import
// useSWR from "swr"` is a top-level static import, eagerly resolved the
// moment this file loads, regardless of which export the importer
// asked for — that must never be true of `./board`).
//
// Every hook below is a thin wrapper: the fetching logic lives exactly
// once, in the plain function it calls, never duplicated here. Cache
// keys always come from `swrKeys` (`../swr-keys`), never a hand-written
// literal — see that file's header comment for why (collision-freedom).
//
// Invalidation rule (mutations only — read hooks never invalidate):
//   - `useCreateBoard` invalidates this model's list — every cached
//     list, regardless of its `query` filter/pagination.
//   - `useUpdateBoard` invalidates the list AND the mutated entity's
//     own detail (both refetch on the next render that reads them).
//   - `useDeleteBoard` invalidates the list AND drops the deleted
//     entity's detail from the cache outright (`revalidate: false` —
//     there is nothing left to refetch).
// This rule is fixed, not configurable per call. A consumer who needs
// different invalidation should call `mutate`/`swrKeys` directly rather
// than reach for these hooks.

import useSWR, { useSWRConfig, type SWRConfiguration, type SWRResponse } from "swr";
import useSWRMutation, {
  type SWRMutationConfiguration,
  type SWRMutationResponse,
} from "swr/mutation";
import type { CratestackRuntime } from "../runtime";
import type { CratestackQueryRequestConfig } from "../queries";
import { swrKeys } from "../swr-keys";
import {
  listBoards,
  getBoard,
  createBoard,
  updateBoard,
  deleteBoard,
  type Board,
  type CreateBoardInput,
  type UpdateBoardInput,
} from "./board";

export function useBoards(
  runtime: CratestackRuntime,
  options: CratestackQueryRequestConfig = {},
  config?: SWRConfiguration<Board[], Error>,
): SWRResponse<Board[], Error> {
  return useSWR(swrKeys.model.Board.list(options.query), () => listBoards(runtime, options), config);
}

// `id` accepts `null`/`undefined` so this hook can be called before its
// argument is known (e.g. a route param that hasn't resolved yet) —
// `swrKeys.model.Board.get` returns `null` in that case, which is
// SWR's conditional-fetching idiom: `useSWR(null, ...)` never fires a
// request, unlike calling getBoard directly with `undefined` would.
export function useBoard(
  runtime: CratestackRuntime,
  id: number | null | undefined,
  options: CratestackQueryRequestConfig = {},
  config?: SWRConfiguration<Board, Error>,
): SWRResponse<Board, Error> {
  return useSWR(
    swrKeys.model.Board.get(id, options.query),
    () => getBoard(runtime, id as number, options),
    config,
  );
}

export function useCreateBoard(
  runtime: CratestackRuntime,
  config?: SWRMutationConfiguration<
    Board,
    Error,
    ReturnType<typeof swrKeys.model.Board.create>,
    CreateBoardInput
  >,
): SWRMutationResponse<
  Board,
  Error,
  ReturnType<typeof swrKeys.model.Board.create>,
  CreateBoardInput
> {
  const { mutate } = useSWRConfig();
  return useSWRMutation(
    swrKeys.model.Board.create(),
    (_key, { arg }: { arg: CreateBoardInput }) => createBoard(runtime, arg),
    {
      ...config,
      onSuccess: (data, key, mutationConfig) => {
        void mutate(swrKeys.model.Board.listMatches);
        config?.onSuccess?.(data, key, mutationConfig);
      },
    },
  );
}

export function useUpdateBoard(
  runtime: CratestackRuntime,
  id: number,
  config?: SWRMutationConfiguration<
    Board,
    Error,
    ReturnType<typeof swrKeys.model.Board.update>,
    UpdateBoardInput
  >,
): SWRMutationResponse<
  Board,
  Error,
  ReturnType<typeof swrKeys.model.Board.update>,
  UpdateBoardInput
> {
  const { mutate } = useSWRConfig();
  return useSWRMutation(
    swrKeys.model.Board.update(id),
    (_key, { arg }: { arg: UpdateBoardInput }) => updateBoard(runtime, id, arg),
    {
      ...config,
      onSuccess: (data, key, mutationConfig) => {
        void mutate(swrKeys.model.Board.listMatches);
        void mutate(swrKeys.model.Board.get(id));
        config?.onSuccess?.(data, key, mutationConfig);
      },
    },
  );
}

export function useDeleteBoard(
  runtime: CratestackRuntime,
  id: number,
  config?: SWRMutationConfiguration<
    void,
    Error,
    ReturnType<typeof swrKeys.model.Board.delete>,
    void
  >,
): SWRMutationResponse<void, Error, ReturnType<typeof swrKeys.model.Board.delete>, void> {
  const { mutate } = useSWRConfig();
  return useSWRMutation(
    swrKeys.model.Board.delete(id),
    () => deleteBoard(runtime, id),
    {
      ...config,
      onSuccess: (data, key, mutationConfig) => {
        void mutate(swrKeys.model.Board.listMatches);
        void mutate(swrKeys.model.Board.get(id), undefined, { revalidate: false });
        config?.onSuccess?.(data, key, mutationConfig);
      },
    },
  );
}