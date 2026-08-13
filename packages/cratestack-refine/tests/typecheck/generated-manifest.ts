// Compile-only. Never imported at runtime, never executed by vitest —
// its whole job is to make `tsc --noEmit -p tsconfig.typecheck.json` fail
// if the generator's emitted `ResourceMap` stops satisfying this
// package's `ResourceConfig` contract.
//
// The assignment below is the assertion. It fails if the generator emits
// a wrong-typed field (`paged: "yes"`), drops a required one, or binds
// `api` to something that no longer structurally satisfies
// `CratestackModelApi` — e.g. if a future codegen change made the
// generated `update`/`delete` optional, or renamed a method.

import type { ResourceMap } from "../../src/types.js";
import type { RefineFixtureClientClient } from "../fixtures/generated-client/src/client.js";
import { cratestackRefineResources } from "../fixtures/generated-client/src/refine.js";

export function generatedManifestIsAResourceMap(client: RefineFixtureClientClient): ResourceMap {
  return cratestackRefineResources(client);
}
