// Compile-only. Never imported at runtime, never executed by vitest — its
// whole job is to make `tsc --noEmit -p tsconfig.typecheck.json` fail if a
// real generated RPC client (`RefineFixtureRpcClientClient`, produced by
// `just refine-rpc-fixture` against `refine_fixture_rpc.cstack`) stops
// structurally satisfying `RpcResourceMap`/`CratestackRpcModelApi`.
//
// `cratestack generate-typescript --refine` DOES now emit a real
// `refine.ts` for `transport rpc` schemas too (issue #571's RPC follow-up,
// #586 — `TypeScriptGeneratorError::RefineRequiresRest` was widened to
// `RefineRequiresRestOrRpc`, and `just refine-rpc-fixture` passes
// `--refine`). That generated manifest's own round trip is checked by the
// sibling file `generated-manifest-rpc.ts`, the RPC equivalent of REST's
// `generated-manifest.ts`.
//
// This file stays alongside it deliberately, hand-building the manifest
// exactly as every RPC example in the README does: it proves the real
// generated RPC model classes are assignable to `CratestackRpcModelApi`
// on their own structural merits, independent of whether the manifest
// generator wires them up correctly — a narrower, complementary claim to
// `generated-manifest-rpc.ts`'s, not a stand-in for it.

import type { RpcResourceMap } from "../../src/rpc-types.js";
import type { RefineFixtureRpcClientClient } from "../fixtures/generated-client-rpc/src/client.js";

export function handWrittenRpcManifestIsAnRpcResourceMap(
  client: RefineFixtureRpcClientClient,
): RpcResourceMap {
  return {
    widgets: { api: client.widgets, primaryKey: "id", paged: false },
    ledgers: { api: client.ledgers, primaryKey: "id", paged: true, versionField: "version" },
    products: { api: client.products, primaryKey: "sku", paged: false },
  };
}
