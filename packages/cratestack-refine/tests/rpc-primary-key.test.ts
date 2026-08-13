import { describe, expect, it } from "vitest";
import { createCratestackRpcDataProvider } from "../src/rpc-provider.js";
import { createRpcTestClient, PRODUCT_RPC_SCHEMA } from "./support/rpc-client.js";

/** RPC sibling of `primary-key.test.ts` — `Product`'s `@id` is `sku`, not
 *  `id`, and the real generated `ProductApi.get`/`.update`/`.delete` all
 *  take a `string` id positionally over RPC exactly like REST does (see
 *  `tests/fixtures/generated-client-rpc/src/client.ts`). */
describe("a model whose @id is not named id, over RPC", () => {
  function setup() {
    const { server, client } = createRpcTestClient([PRODUCT_RPC_SCHEMA]);
    server.seed("Product", { sku: "SKU-1", name: "Widget Deluxe" });
    const provider = createCratestackRpcDataProvider({
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
    server.seed("Product", { sku: "SKU-2", name: "Gadget" });
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
    expect(server.row("Product", "SKU-1")).toMatchObject({ name: "Widget Deluxe II" });
  });

  it("deleteOne passes refine's id straight through as the real sku", async () => {
    const { server, provider } = setup();
    await provider.deleteOne({ resource: "products", id: "SKU-1" });
    expect(server.row("Product", "SKU-1")).toBeUndefined();
  });

  it("create uses the schema's real primary-key field name (sku), not id", async () => {
    const { server, provider } = setup();
    const result = await provider.create({
      resource: "products",
      variables: { sku: "SKU-3", name: "Thingamajig" },
    });
    expect(result.data).toMatchObject({ sku: "SKU-3", id: "SKU-3" });
    expect(server.row("Product", "SKU-3")).toMatchObject({ sku: "SKU-3", name: "Thingamajig" });
  });
});
