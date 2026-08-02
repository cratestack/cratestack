// Generated SWR hooks for this model (issue #305) — a sibling file to
// `./task` (the plain, framework-free functions from issue
// #304), not appended into it: see `cratestack-client-typescript`'s
// `src/swr/mod.rs` module doc for why hooks and plain functions can't
// share a file without breaking the framework-free guarantee (`import
// useSWR from "swr"` is a top-level static import, eagerly resolved the
// moment this file loads, regardless of which export the importer
// asked for — that must never be true of `./task`).
//
// Every hook below is a thin wrapper: the fetching logic lives exactly
// once, in the plain function it calls, never duplicated here. Cache
// keys always come from `swrKeys` (`../swr-keys`), never a hand-written
// literal — see that file's header comment for why (collision-freedom).
//
// Invalidation rule (mutations only — read hooks never invalidate):
//   - `useCreateTask` invalidates this model's list — every cached
//     list, regardless of its `query` filter/pagination.
//   - `useUpdateTask` invalidates the list AND the mutated entity's
//     own detail (both refetch on the next render that reads them).
//   - `useDeleteTask` invalidates the list AND drops the deleted
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
  listTasks,
  getTask,
  createTask,
  updateTask,
  deleteTask,
  type Task,
  type CreateTaskInput,
  type UpdateTaskInput,
} from "./task";

export function useTasks(
  runtime: CratestackRuntime,
  options: CratestackQueryRequestConfig = {},
  config?: SWRConfiguration<Task[], Error>,
): SWRResponse<Task[], Error> {
  return useSWR(swrKeys.model.Task.list(options.query), () => listTasks(runtime, options), config);
}

// `id` accepts `null`/`undefined` so this hook can be called before its
// argument is known (e.g. a route param that hasn't resolved yet) —
// `swrKeys.model.Task.get` returns `null` in that case, which is
// SWR's conditional-fetching idiom: `useSWR(null, ...)` never fires a
// request, unlike calling getTask directly with `undefined` would.
export function useTask(
  runtime: CratestackRuntime,
  id: number | null | undefined,
  options: CratestackQueryRequestConfig = {},
  config?: SWRConfiguration<Task, Error>,
): SWRResponse<Task, Error> {
  return useSWR(
    swrKeys.model.Task.get(id, options.query),
    () => getTask(runtime, id as number, options),
    config,
  );
}

export function useCreateTask(
  runtime: CratestackRuntime,
  config?: SWRMutationConfiguration<
    Task,
    Error,
    ReturnType<typeof swrKeys.model.Task.create>,
    CreateTaskInput
  >,
): SWRMutationResponse<
  Task,
  Error,
  ReturnType<typeof swrKeys.model.Task.create>,
  CreateTaskInput
> {
  const { mutate } = useSWRConfig();
  return useSWRMutation(
    swrKeys.model.Task.create(),
    (_key, { arg }: { arg: CreateTaskInput }) => createTask(runtime, arg),
    {
      ...config,
      onSuccess: (data, key, mutationConfig) => {
        void mutate(swrKeys.model.Task.listMatches);
        config?.onSuccess?.(data, key, mutationConfig);
      },
    },
  );
}

export function useUpdateTask(
  runtime: CratestackRuntime,
  id: number,
  config?: SWRMutationConfiguration<
    Task,
    Error,
    ReturnType<typeof swrKeys.model.Task.update>,
    UpdateTaskInput
  >,
): SWRMutationResponse<
  Task,
  Error,
  ReturnType<typeof swrKeys.model.Task.update>,
  UpdateTaskInput
> {
  const { mutate } = useSWRConfig();
  return useSWRMutation(
    swrKeys.model.Task.update(id),
    (_key, { arg }: { arg: UpdateTaskInput }) => updateTask(runtime, id, arg),
    {
      ...config,
      onSuccess: (data, key, mutationConfig) => {
        void mutate(swrKeys.model.Task.listMatches);
        void mutate(swrKeys.model.Task.get(id));
        config?.onSuccess?.(data, key, mutationConfig);
      },
    },
  );
}

export function useDeleteTask(
  runtime: CratestackRuntime,
  id: number,
  config?: SWRMutationConfiguration<
    void,
    Error,
    ReturnType<typeof swrKeys.model.Task.delete>,
    void
  >,
): SWRMutationResponse<void, Error, ReturnType<typeof swrKeys.model.Task.delete>, void> {
  const { mutate } = useSWRConfig();
  return useSWRMutation(
    swrKeys.model.Task.delete(id),
    () => deleteTask(runtime, id),
    {
      ...config,
      onSuccess: (data, key, mutationConfig) => {
        void mutate(swrKeys.model.Task.listMatches);
        void mutate(swrKeys.model.Task.get(id), undefined, { revalidate: false });
        config?.onSuccess?.(data, key, mutationConfig);
      },
    },
  );
}