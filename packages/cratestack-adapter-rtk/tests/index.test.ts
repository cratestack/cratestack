import { describe, expect, it, vi } from "vitest";
import { createRpcBaseQuery, type RpcCaller } from "../src/index.js";

function fakeApi(signal: AbortSignal) {
  return { signal } as Parameters<ReturnType<typeof createRpcBaseQuery>>[1];
}

describe("createRpcBaseQuery", () => {
  it("resolves { data } on a successful call", async () => {
    const call = vi.fn(async () => ({ id: 1, name: "widget" }));
    const client: RpcCaller = { call: call as unknown as RpcCaller["call"] };
    const baseQuery = createRpcBaseQuery(client);
    const controller = new AbortController();

    const result = await baseQuery(
      { opId: "model.Widget.get", input: { id: 1 } },
      fakeApi(controller.signal),
      {},
    );

    expect(result).toEqual({ data: { id: 1, name: "widget" } });
    expect(call).toHaveBeenCalledWith("model.Widget.get", { id: 1 }, { signal: controller.signal });
  });

  it("defaults a missing input to null", async () => {
    const call = vi.fn(async () => "pong");
    const client: RpcCaller = { call: call as unknown as RpcCaller["call"] };
    const baseQuery = createRpcBaseQuery(client);

    await baseQuery({ opId: "procedure.ping" }, fakeApi(new AbortController().signal), {});

    expect(call).toHaveBeenCalledWith("procedure.ping", null, expect.anything());
  });

  it("maps a structured RpcErrorBody-shaped rejection to status RPC_ERROR", async () => {
    const call = vi.fn(async () => {
      throw { code: "not_found", message: "widget 1 not found" };
    });
    const client: RpcCaller = { call: call as unknown as RpcCaller["call"] };
    const baseQuery = createRpcBaseQuery(client);

    const result = await baseQuery(
      { opId: "model.Widget.get", input: { id: 1 } },
      fakeApi(new AbortController().signal),
      {},
    );

    expect(result).toEqual({
      error: {
        status: "RPC_ERROR",
        error: "widget 1 not found",
        data: { code: "not_found", message: "widget 1 not found" },
      },
    });
  });

  it("maps a plain network failure to status RPC_TRANSPORT_ERROR", async () => {
    const call = vi.fn(async () => {
      throw new Error("network down");
    });
    const client: RpcCaller = { call: call as unknown as RpcCaller["call"] };
    const baseQuery = createRpcBaseQuery(client);

    const result = await baseQuery(
      { opId: "procedure.ping" },
      fakeApi(new AbortController().signal),
      {},
    );

    expect(result).toEqual({ error: { status: "RPC_TRANSPORT_ERROR", error: "network down" } });
  });
});
