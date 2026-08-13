import { describe, expect, it } from "vitest";
import { createCratestackDataProvider } from "../src/index.js";
import { cratestackRefineResources } from "./fixtures/generated-client/src/refine.js";
import {
  createTestClient,
  LEDGER_SCHEMA,
  PRODUCT_SCHEMA,
  WIDGET_SCHEMA,
} from "./support/client.js";

/** `cratestack generate-typescript --refine` (cratestack#571) emits the
 *  `ResourceMap` this package otherwise asks consumers to hand-write.
 *
 *  This file covers the *runtime* half: the generated manifest carries
 *  the same four facts the hand-written manifests in the sibling test
 *  files carry, read off the same `refine_fixture.cstack` — plus a real
 *  round trip through the provider, so the `api` binding is exercised
 *  rather than merely inspected.
 *
 *  The *compile-time* half is `tests/typecheck/generated-manifest.ts`,
 *  checked by `tsc --noEmit -p tsconfig.typecheck.json` (the first half
 *  of this package's `test` script). That is what proves
 *  `cratestackRefineResources(client)` is assignable to `ResourceMap` at
 *  all — i.e. that every generated model API structurally satisfies
 *  `CratestackModelApi`. `vitest` transpiles without type-checking, so
 *  nothing in *this* file could establish it, and a string assertion over
 *  the generated source could never tell "satisfies the interface" from
 *  "contains the right words".
 */
describe("the generated --refine manifest", () => {
  function setup() {
    const { server, client } = createTestClient([WIDGET_SCHEMA, LEDGER_SCHEMA, PRODUCT_SCHEMA]);
    return { server, resources: cratestackRefineResources(client) };
  }

  it("names one resource per model, keyed by the client's own accessor", () => {
    const { resources } = setup();
    expect(Object.keys(resources).sort()).toEqual(["ledgers", "products", "widgets"]);
  });

  it("reads @id off the schema rather than assuming `id`", () => {
    const { resources } = setup();
    expect(resources.widgets?.primaryKey).toBe("id");
    expect(resources.ledgers?.primaryKey).toBe("id");
    // Product's @id is `sku` — the case a hand-written manifest gets
    // wrong silently.
    expect(resources.products?.primaryKey).toBe("sku");
  });

  it("marks only the @@paged model as paged", () => {
    const { resources } = setup();
    expect(resources.ledgers?.paged).toBe(true);
    expect(resources.widgets?.paged).toBe(false);
    expect(resources.products?.paged).toBe(false);
  });

  it("sets versionField only for the @version model, and omits the key otherwise", () => {
    const { resources } = setup();
    expect(resources.ledgers?.versionField).toBe("version");
    // Omitted entirely, not set to undefined — `ResourceConfig` is
    // consumed under `exactOptionalPropertyTypes`, where the two differ.
    expect(resources.widgets && "versionField" in resources.widgets).toBe(false);
    expect(resources.products && "versionField" in resources.products).toBe(false);
  });

  it("drives a real round trip through the provider it was built for", async () => {
    const { server, resources } = setup();
    server.seed("products", { sku: "SKU-1", name: "Widget Deluxe" });
    const provider = createCratestackDataProvider(resources);

    const one = await provider.getOne({ resource: "products", id: "SKU-1" });
    // `id` is refine's synthetic key, mapped off the generated
    // `primaryKey: "sku"` — so this asserts the generated fact was
    // actually used, not just present.
    expect(one.data).toMatchObject({ sku: "SKU-1", id: "SKU-1" });
  });

  it("reports a real total for the @@paged model, via the generated paged flag", async () => {
    const { server, resources } = setup();
    server.seed("ledgers", { id: 1, label: "opening", balance: 100, version: 1 });
    server.seed("ledgers", { id: 2, label: "closing", balance: 250, version: 1 });
    const provider = createCratestackDataProvider(resources);

    const list = await provider.getList({ resource: "ledgers" });
    expect(list.total).toBe(2);
  });
});
