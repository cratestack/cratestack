// Compile-only. Never imported at runtime, never executed by vitest — its
// whole job is to make `tsc --noEmit -p tsconfig.typecheck.json` fail if a
// real generated RPC client (`RefineFixtureRpcClientClient`, produced by
// `just refine-rpc-fixture` against `refine_fixture_rpc.cstack`) stops
// structurally satisfying `RpcResourceMap`/`CratestackRpcModelApi`.
//
// There is no generated `refine.ts` to assign here the way
// `generated-manifest.ts` does for REST — `cratestack generate-typescript
// --refine` still rejects `transport rpc` schemas
// (`TypeScriptGeneratorError::RefineRequiresRest`), so this package's own
// tests build the manifest by hand, exactly as every RPC example in the
// README does. This file's assertion is narrower than the REST one as a
// result: it proves the real generated RPC model classes are assignable
// to `CratestackRpcModelApi`, not that a generator emits a correct
// mapping (there is no such generator output to check yet).

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
