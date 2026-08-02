import { FakeRuntime } from "@cratestack/ts-types/test-harness";
import { describe, expect, it, vi } from "vitest";
import { createLoggerLink } from "../src/index.js";

function okResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), { status: 200 });
}

describe("createLoggerLink", () => {
  it("logs the outgoing call and the response status on success", async () => {
    const logger = { info: vi.fn(), error: vi.fn() };
    const fetchMock = vi.fn(async () => okResponse({ ok: true }));
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [
      createLoggerLink(logger),
    ]);

    await runtime.call("model.Widget.get", { id: 1 });

    expect(logger.info).toHaveBeenCalledWith(expect.stringContaining("-> unary model.Widget.get"));
    expect(logger.info).toHaveBeenCalledWith(expect.stringContaining("<- model.Widget.get 200"));
    expect(logger.error).not.toHaveBeenCalled();
  });

  it("logs and rethrows when the next link throws", async () => {
    const logger = { info: vi.fn(), error: vi.fn() };
    const failure = new Error("network down");
    const fetchMock = vi.fn(async () => {
      throw failure;
    });
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [
      createLoggerLink(logger),
    ]);

    await expect(runtime.call("procedure.echo", null)).rejects.toThrow(failure);

    expect(logger.error).toHaveBeenCalledWith(
      expect.stringContaining("x procedure.echo failed"),
      failure,
    );
  });

  it("defaults to the global console when no logger is passed", async () => {
    const fetchMock = vi.fn(async () => okResponse({ ok: true }));
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createLoggerLink()]);

    await expect(runtime.call("procedure.echo", null)).resolves.toEqual({ ok: true });
  });
});
