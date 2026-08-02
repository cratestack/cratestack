import { FakeRuntime } from "@cratestack/ts-types/test-harness";
import { describe, expect, it, vi } from "vitest";
import { z } from "zod";
import { createZodValidatorLink } from "../src/index.js";

function okResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), { status: 200 });
}

describe("createZodValidatorLink", () => {
  it("passes ops with no configured schema straight through", async () => {
    const fetchMock = vi.fn(async () => okResponse({ ok: true }));
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [
      createZodValidatorLink({}),
    ]);

    await runtime.call("procedure.echo", { anything: "goes" });

    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("forwards the parsed (coerced) input to next() on success", async () => {
    const fetchMock = vi.fn(async (_url: string, init: RequestInit) => {
      expect(JSON.parse(init.body as string)).toEqual({ id: 42 });
      return okResponse({ ok: true });
    });
    const link = createZodValidatorLink({
      "model.Widget.get": z.object({ id: z.coerce.number() }),
    });
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [link]);

    await runtime.call("model.Widget.get", { id: "42" });

    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("short-circuits with a 400 invalid_argument error and never calls next() on failure", async () => {
    const fetchMock = vi.fn(async () => okResponse({ ok: true }));
    const link = createZodValidatorLink({
      "model.Order.create": z.object({ total: z.number().positive() }),
    });
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [link]);

    await expect(runtime.call("model.Order.create", { total: -5 })).rejects.toThrow(
      /invalid_argument/,
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
