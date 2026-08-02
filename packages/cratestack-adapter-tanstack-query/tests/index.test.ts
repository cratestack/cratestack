import { describe, expect, it, vi } from "vitest";
import {
  type RpcCaller,
  isRpcErrorCode,
  rpcMutationOptions,
  rpcQueryKey,
  rpcQueryOptions,
} from "../src/index.js";

function fakeClient(call: RpcCaller["call"]): RpcCaller {
  return { call };
}

describe("rpcQueryKey", () => {
  it("includes input when provided", () => {
    expect(rpcQueryKey("model.Widget.get", { id: 1 })).toEqual(["model.Widget.get", { id: 1 }]);
  });

  it("omits input entirely when undefined, rather than a trailing undefined slot", () => {
    expect(rpcQueryKey("procedure.ping")).toEqual(["procedure.ping"]);
  });
});

describe("rpcQueryOptions", () => {
  it("builds a queryKey/queryFn pair that calls through to the client", async () => {
    const call = vi.fn(async (opId: string, input: unknown) => ({ opId, input }));
    const client = fakeClient(call as unknown as RpcCaller["call"]);
    const options = rpcQueryOptions(client, "model.Widget.get", { id: 1 });

    expect(options.queryKey).toEqual(["model.Widget.get", { id: 1 }]);
    const controller = new AbortController();
    const result = await options.queryFn({ signal: controller.signal });

    expect(result).toEqual({ opId: "model.Widget.get", input: { id: 1 } });
    expect(call).toHaveBeenCalledWith("model.Widget.get", { id: 1 }, { signal: controller.signal });
  });
});

describe("rpcMutationOptions", () => {
  it("builds a mutationKey/mutationFn pair that takes input at call time", async () => {
    const call = vi.fn(async (opId: string, input: unknown) => ({ opId, input }));
    const client = fakeClient(call as unknown as RpcCaller["call"]);
    const options = rpcMutationOptions(client, "model.Order.create");

    expect(options.mutationKey).toEqual(["model.Order.create"]);
    const result = await options.mutationFn({ total: 10 });

    expect(result).toEqual({ opId: "model.Order.create", input: { total: 10 } });
  });
});

describe("isRpcErrorCode", () => {
  it("matches a structurally-shaped RpcErrorBody by code", () => {
    expect(isRpcErrorCode({ code: "not_found", message: "nope" }, "not_found")).toBe(true);
    expect(isRpcErrorCode({ code: "conflict", message: "nope" }, "not_found")).toBe(false);
  });

  it("is false for non-object errors", () => {
    expect(isRpcErrorCode(new Error("boom"), "not_found")).toBe(false);
    expect(isRpcErrorCode(null, "not_found")).toBe(false);
  });
});
