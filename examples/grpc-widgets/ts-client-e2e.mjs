// Ticket #172's load-bearing integration test: generate a real TS client
// (`ts-client/`, generated from `schemas/widgets.cstack` via `cratestack
// generate-typescript`), boot the real `grpc-widgets-example` server
// (ticket #171's `grpcurl`-verified example, unmodified), and drive it
// from real Node.js `fetch` through the generated gRPC-Web client and
// runtime — no mocks, no `grpcurl`.
//
// Run:
//   1. `DATABASE_URL=postgres://cratestack:cratestack@localhost:55432/cratestack_test \
//        cargo run -p grpc-widgets-example` (separate shell)
//   2. `cd examples/grpc-widgets/ts-client && npm install && npm run build`
//   3. `node examples/grpc-widgets/ts-client-e2e.mjs`

import assert from "node:assert/strict";
import {
  CratestackExamplesWidgetsGrpcClientClient,
} from "./ts-client/src/client.ts";
import { CratestackGrpcError } from "./ts-client/src/runtime.ts";

const ORIGIN = "http://127.0.0.1:50061";
const AUTH_HEADERS = { "x-auth-id": "1" };

async function main() {
  const client = new CratestackExamplesWidgetsGrpcClientClient(ORIGIN, {
    headers: AUTH_HEADERS,
  });

  // --- CORS: the single highest-severity failure mode per
  // docs/design/protobuf.md §7.4 point 2 — assert it directly against a
  // real cross-origin-shaped request before trusting anything else the
  // client reports.
  const corsProbe = await fetch(`${ORIGIN}/widgets_api.Api/ModelWidgetList`, {
    method: "POST",
    headers: {
      ...AUTH_HEADERS,
      "content-type": "application/grpc-web+proto",
      origin: "http://example.com",
    },
    body: new Uint8Array([0, 0, 0, 0, 0]), // empty WidgetRpcListInput, framed
  });
  const exposed = corsProbe.headers.get("access-control-expose-headers") ?? "";
  for (const name of ["grpc-status", "grpc-message", "grpc-status-details-bin"]) {
    assert.ok(exposed.includes(name), `expected '${name}' in Access-Control-Expose-Headers, got '${exposed}'`);
  }
  console.log("[ok] CORS: Access-Control-Expose-Headers =", exposed);

  // --- create
  const created = await client.widgets.create({ id: Date.now() % 1_000_000, name: "gizmo" });
  assert.equal(created.name, "gizmo");
  assert.equal(typeof created.id, "number");
  console.log("[ok] create ->", created);

  // --- get
  const fetched = await client.widgets.get(created.id);
  assert.deepEqual(fetched, created);
  console.log("[ok] get ->", fetched);

  // --- list
  const page = await client.widgets.list({ limit: 50 });
  assert.ok(Array.isArray(page.items));
  assert.ok(page.items.some((item) => item.id === created.id));
  assert.equal(typeof page.pageInfo.hasNextPage, "boolean");
  console.log(`[ok] list -> ${page.items.length} item(s), pageInfo =`, page.pageInfo);

  // --- update
  const updated = await client.widgets.update(created.id, { name: "gizmo-v2" });
  assert.equal(updated.name, "gizmo-v2");
  assert.equal(updated.id, created.id);
  console.log("[ok] update ->", updated);

  // --- delete
  await client.widgets.delete(created.id);
  console.log("[ok] delete -> (void)");

  // --- deliberate error: get-after-delete must surface a typed,
  // CORS-readable NOT_FOUND, not a silent success or an opaque failure.
  let threw = false;
  try {
    await client.widgets.get(created.id);
  } catch (error) {
    threw = true;
    assert.ok(error instanceof CratestackGrpcError, `expected CratestackGrpcError, got ${error}`);
    assert.equal(error.code, "not_found", `expected code 'not_found', got '${error.code}'`);
    console.log("[ok] get-after-delete ->", { status: error.status, code: error.code, message: error.message });
  }
  assert.ok(threw, "get-after-delete should have thrown");

  console.log("\nAll gRPC-Web TS client checks passed.");
}

main().catch((error) => {
  console.error("FAILED:", error);
  process.exitCode = 1;
});
