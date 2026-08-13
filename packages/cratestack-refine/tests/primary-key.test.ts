import { describe, expect, it } from "vitest";
import { createCratestackDataProvider } from "../src/index.js";
import { createTestClient, PRODUCT_SCHEMA } from "./support/client.js";

/** `Product`'s `@id` is `sku`, not `id` — the real generated
 *  `ProductApi.get`/`.update`/`.delete` all take a `string` id
 *  positionally (see `tests/fixtures/generated-client/src/client.ts`),
 *  proving this isn't just a type-level relabeling. */
describe("a model whose @id is not named id", () => {
  function setup() {
    const { server, client } = createTestClient([PRODUCT_SCHEMA]);
    server.seed("products", { sku: "SKU-1", name: "Widget Deluxe" });
    const provider = createCratestackDataProvider({
      products: { api: client.products, primaryKey: "sku", paged: false },
    });
    return { server, provider };
  }

  it("getOne attaches a synthetic id equal to the real primary key's value", async () => {
    const { provider } = setup();
    const result = await provider.getOne({ resource: "products", id: "SKU-1" });
    expect(result.data).toMatchObject({ sku: "SKU-1", name: "Widget Deluxe", id: "SKU-1" });
  });

  it("getList attaches id to every row, keyed off the configured primaryKey", async () => {
    const { server, provider } = setup();
    server.seed("products", { sku: "SKU-2", name: "Gadget" });
    const result = await provider.getList({ resource: "products" });
    expect(result.data.map((row) => row.id).sort()).toEqual(["SKU-1", "SKU-2"]);
    void server;
  });

  it("update passes refine's id straight through as the real sku, unaltered", async () => {
    const { server, provider } = setup();
    await provider.update({
      resource: "products",
      id: "SKU-1",
      variables: { name: "Widget Deluxe II" },
    });
    expect(server.row("products", "SKU-1")).toMatchObject({ name: "Widget Deluxe II" });
  });

  it("deleteOne passes refine's id straight through as the real sku", async () => {
    const { server, provider } = setup();
    await provider.deleteOne({ resource: "products", id: "SKU-1" });
    expect(server.row("products", "SKU-1")).toBeUndefined();
  });

  it("create uses the schema's real primary-key field name (sku), not id", async () => {
    const { server, provider } = setup();
    const result = await provider.create({
      resource: "products",
      variables: { sku: "SKU-3", name: "Thingamajig" },
    });
    expect(result.data).toMatchObject({ sku: "SKU-3", id: "SKU-3" });
    expect(server.row("products", "SKU-3")).toMatchObject({ sku: "SKU-3", name: "Thingamajig" });
  });
});
