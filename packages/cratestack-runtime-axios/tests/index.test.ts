import type { AxiosInstance, AxiosRequestConfig } from "axios";
import { describe, expect, it, vi } from "vitest";
import { createAxiosRuntime } from "../src/index.js";

function fakeInstance(request: (config: AxiosRequestConfig) => Promise<unknown>): AxiosInstance {
  return { request } as unknown as AxiosInstance;
}

describe("createAxiosRuntime", () => {
  it("translates a fetch-style call into an axios request and back into a Response", async () => {
    const request = vi.fn(async (config: AxiosRequestConfig) => {
      expect(config.url).toBe("https://example.test/rpc/procedure.echo");
      expect(config.method).toBe("POST");
      expect(config.headers).toEqual({ "content-type": "application/json" });
      expect(config.data).toBe('{"hi":true}');
      return {
        status: 200,
        statusText: "OK",
        headers: { "content-type": "application/json" },
        data: new TextEncoder().encode('{"ok":true}').buffer,
      };
    });
    const fetchFn = createAxiosRuntime({ instance: fakeInstance(request) });

    const response = await fetchFn("https://example.test/rpc/procedure.echo", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: '{"hi":true}',
    });

    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toBe("application/json");
    expect(await response.json()).toEqual({ ok: true });
  });

  it("never throws on a non-2xx status — surfaces it as a normal Response", async () => {
    const request = vi.fn(async () => ({
      status: 404,
      statusText: "Not Found",
      headers: {},
      data: new TextEncoder().encode('{"code":"not_found","message":"nope"}').buffer,
    }));
    const fetchFn = createAxiosRuntime({ instance: fakeInstance(request) });

    const response = await fetchFn("https://example.test/rpc/model.Widget.get", { method: "POST" });

    expect(response.ok).toBe(false);
    expect(response.status).toBe(404);
    expect(await response.json()).toEqual({ code: "not_found", message: "nope" });
  });

  it("defaults to GET and empty headers when init is omitted", async () => {
    const request = vi.fn(async (config: AxiosRequestConfig) => {
      expect(config.method).toBe("GET");
      expect(config.headers).toEqual({});
      return { status: 204, statusText: "No Content", headers: {}, data: new ArrayBuffer(0) };
    });
    const fetchFn = createAxiosRuntime({ instance: fakeInstance(request) });

    await fetchFn("https://example.test/health");

    expect(request).toHaveBeenCalledTimes(1);
  });

  it("carries a Request object's own method/headers/body through when no init is given", async () => {
    const request = vi.fn(async (config: AxiosRequestConfig) => {
      expect(config.url).toBe("https://example.test/rpc/procedure.echo");
      expect(config.method).toBe("POST");
      expect(config.headers).toEqual({ "content-type": "application/json" });
      expect(await new Response(config.data as ArrayBuffer).text()).toBe('{"hi":true}');
      return { status: 204, statusText: "No Content", headers: {}, data: new ArrayBuffer(0) };
    });
    const fetchFn = createAxiosRuntime({ instance: fakeInstance(request) });
    const req = new Request("https://example.test/rpc/procedure.echo", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: '{"hi":true}',
    });

    await fetchFn(req);

    expect(request).toHaveBeenCalledTimes(1);
  });

  it("lets an explicit init field override the same field on a Request object", async () => {
    const request = vi.fn(async (config: AxiosRequestConfig) => {
      expect(config.method).toBe("DELETE");
      return { status: 204, statusText: "No Content", headers: {}, data: new ArrayBuffer(0) };
    });
    const fetchFn = createAxiosRuntime({ instance: fakeInstance(request) });
    const req = new Request("https://example.test/rpc/procedure.echo", { method: "POST" });

    await fetchFn(req, { method: "DELETE" });

    expect(request).toHaveBeenCalledTimes(1);
  });
});
