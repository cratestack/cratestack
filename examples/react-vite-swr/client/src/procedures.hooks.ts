// Generated SWR hooks for this schema's procedures (issue #305) — a
// sibling file to `./procedures` (the plain, framework-free functions
// from issue #304), not appended into it: see
// `cratestack-client-typescript`'s `src/swr/mod.rs` module doc for why
// hooks and plain functions can't share a file without breaking the
// framework-free guarantee.
//
// Every hook below is a thin wrapper: the fetching logic lives exactly
// once, in the plain function it calls, never duplicated here. Cache
// keys always come from `swrKeys` (`./swr-keys`), never a hand-written
// literal. A query-kind procedure gets a `useSWR` hook whose `args`
// parameter accepts `null`/`undefined` — SWR's conditional-fetching
// idiom (`useSWR(null, ...)` never fires) — for the same "argument not
// known yet" case model detail hooks handle. A mutation-kind procedure
// gets a `useSWRMutation` hook; procedures have no list/detail to
// invalidate (that's model CRUD's job — see `src/models/*.hooks.ts`'s
// own invalidation-rule comment), so these never call `mutate`.

import useSWR, { type SWRConfiguration, type SWRResponse } from "swr";
import useSWRMutation, {
  type SWRMutationConfiguration,
  type SWRMutationResponse,
} from "swr/mutation";
import type { CratestackRuntime } from "./runtime";
import { swrKeys } from "./swr-keys";
import {
  estimateFocusMinutes,
  type EstimateFocusMinutesArgs,
  type FocusEstimateArgs,
  type FocusEstimateResult,
} from "./procedures";

export function useEstimateFocusMinutesQuery(
  runtime: CratestackRuntime,
  args: EstimateFocusMinutesArgs | null | undefined,
  config?: SWRConfiguration<FocusEstimateResult, Error>,
): SWRResponse<FocusEstimateResult, Error> {
  return useSWR(
    swrKeys.procedure.estimateFocusMinutes(args),
    () => estimateFocusMinutes(runtime, args as EstimateFocusMinutesArgs),
    config,
  );
}

