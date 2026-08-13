// Compile-only. Never imported at runtime, never executed by vitest —
// its whole job is to make `tsc --noEmit -p tsconfig.typecheck.json` fail
// if the generator's emitted `RpcResourceMap` stops satisfying this
// package's `RpcResourceConfig` contract.
//
// RPC sibling of `generated-manifest.ts` (REST). `cratestack
// generate-typescript --refine` has emitted a real `src/refine.ts` for
// `transport rpc` schemas since issue #571's RPC follow-up (#586) —
// `just refine-rpc-fixture` now passes `--refine`, same as `refine-fixture`
// does for REST. Before that, nothing in this package typechecked the
// generator's RPC output at all: `rpc-manifest.ts` (sibling file) proves
// the generated model classes satisfy `CratestackRpcModelApi`, but that is
// a narrower claim than "the generator's own manifest satisfies
// `RpcResourceMap`" — this file is the direct round-trip equivalent of
// REST's `generated-manifest.ts`, closing that gap.
//
// The assignment below is the assertion. It fails if the generator emits
// a wrong-typed field (`paged: "yes"`), drops a required one, or binds
// `api` to something that no longer structurally satisfies
// `CratestackRpcModelApi`.

import type { RpcResourceMap } from "../../src/rpc-types.js";
import type { RefineFixtureRpcClientClient } from "../fixtures/generated-client-rpc/src/client.js";
import { cratestackRefineResources } from "../fixtures/generated-client-rpc/src/refine.js";

export function generatedRpcManifestIsAnRpcResourceMap(
  client: RefineFixtureRpcClientClient,
): RpcResourceMap {
  return cratestackRefineResources(client);
}
