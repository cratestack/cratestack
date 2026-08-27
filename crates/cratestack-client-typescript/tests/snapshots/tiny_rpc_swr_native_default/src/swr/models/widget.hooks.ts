// Generated SWR hooks for this model (issue #305) — a sibling file to
// `./widget` (the plain, framework-free functions from issue
// #304), not appended into it: see `cratestack-client-typescript`'s
// `src/swr/mod.rs` module doc for why hooks and plain functions can't
// share a file without breaking the framework-free guarantee (`import
// useSWR from "swr"` is a top-level static import, eagerly resolved the
// moment this file loads, regardless of which export the importer
// asked for — that must never be true of `./widget`).
//
// Every hook below is a thin wrapper: the fetching logic lives exactly
// once, in the plain function it calls, never duplicated here. Cache
// keys always come from `swrKeys` (`../swr-keys`), never a hand-written
// literal — see that file's header comment for why (collision-freedom).
//
// Invalidation rule (mutations only — read hooks never invalidate):
//   - `useCreateWidget` invalidates this model's list — every cached
//     list, regardless of its `input` filter/pagination.
//   - `useUpdateWidget` invalidates the list AND the mutated entity's
//     own detail (both refetch on the next render that reads them).
//   - `useDeleteWidget` invalidates the list AND drops the deleted
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
import type { CratestackRpcRuntime } from "../runtime.js";
import type { CratestackRpcListQuery } from "../queries.js";
import { swrKeys } from "../swr-keys.js";
import {
  listWidgets,
  getWidget,
  createWidget,
  updateWidget,
  deleteWidget,
  type Widget,
  type CreateWidgetInput,
  type UpdateWidgetInput,
} from "./widget.js";

export function useWidgets(
  runtime: CratestackRpcRuntime,
  query: CratestackRpcListQuery = {},
  config?: SWRConfiguration<Widget[], Error>,
): SWRResponse<Widget[], Error> {
  return useSWR(swrKeys.model.Widget.list(query), () => listWidgets(runtime, query), config);
}

// `id` accepts `null`/`undefined` so this hook can be called before its
// argument is known (e.g. a route param that hasn't resolved yet) —
// `swrKeys.model.Widget.get` returns `null` in that case, which is
// SWR's conditional-fetching idiom: `useSWR(null, ...)` never fires a
// request, unlike calling getWidget directly with `undefined` would.
export function useWidget(
  runtime: CratestackRpcRuntime,
  id: number | null | undefined,
  config?: SWRConfiguration<Widget, Error>,
): SWRResponse<Widget, Error> {
  return useSWR(
    swrKeys.model.Widget.get(id),
    () => getWidget(runtime, id as number),
    config,
  );
}

export function useCreateWidget(
  runtime: CratestackRpcRuntime,
  config?: SWRMutationConfiguration<
    Widget,
    Error,
    ReturnType<typeof swrKeys.model.Widget.create>,
    CreateWidgetInput
  >,
): SWRMutationResponse<
  Widget,
  Error,
  ReturnType<typeof swrKeys.model.Widget.create>,
  CreateWidgetInput
> {
  const { mutate } = useSWRConfig();
  return useSWRMutation(
    swrKeys.model.Widget.create(),
    (_key, { arg }: { arg: CreateWidgetInput }) => createWidget(runtime, arg),
    {
      ...config,
      onSuccess: (data, key, mutationConfig) => {
        void mutate(swrKeys.model.Widget.listMatches);
        config?.onSuccess?.(data, key, mutationConfig);
      },
    },
  );
}

export function useUpdateWidget(
  runtime: CratestackRpcRuntime,
  id: number,
  config?: SWRMutationConfiguration<
    Widget,
    Error,
    ReturnType<typeof swrKeys.model.Widget.update>,
    UpdateWidgetInput
  >,
): SWRMutationResponse<
  Widget,
  Error,
  ReturnType<typeof swrKeys.model.Widget.update>,
  UpdateWidgetInput
> {
  const { mutate } = useSWRConfig();
  return useSWRMutation(
    swrKeys.model.Widget.update(id),
    (_key, { arg }: { arg: UpdateWidgetInput }) => updateWidget(runtime, id, arg),
    {
      ...config,
      onSuccess: (data, key, mutationConfig) => {
        void mutate(swrKeys.model.Widget.listMatches);
        void mutate(swrKeys.model.Widget.get(id));
        config?.onSuccess?.(data, key, mutationConfig);
      },
    },
  );
}

export function useDeleteWidget(
  runtime: CratestackRpcRuntime,
  id: number,
  config?: SWRMutationConfiguration<
    void,
    Error,
    ReturnType<typeof swrKeys.model.Widget.delete>,
    void
  >,
): SWRMutationResponse<void, Error, ReturnType<typeof swrKeys.model.Widget.delete>, void> {
  const { mutate } = useSWRConfig();
  return useSWRMutation(
    swrKeys.model.Widget.delete(id),
    () => deleteWidget(runtime, id),
    {
      ...config,
      onSuccess: (data, key, mutationConfig) => {
        void mutate(swrKeys.model.Widget.listMatches);
        void mutate(swrKeys.model.Widget.get(id), undefined, { revalidate: false });
        config?.onSuccess?.(data, key, mutationConfig);
      },
    },
  );
}