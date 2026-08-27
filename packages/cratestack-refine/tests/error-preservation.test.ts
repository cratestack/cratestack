import { describe, expect, it } from "vitest";
import { createCratestackDataProvider, toRefineError } from "../src/index.js";
import { createCratestackRpcDataProvider } from "../src/rpc-provider.js";
import { RefineFixtureClientClient } from "./fixtures/generated-client/src/client.js";
import { RefineFixtureRpcClientClient } from "./fixtures/generated-client-rpc/src/client.js";

/** cratestack#786: the data provider used to handle errors two different
 *  ways depending on which method threw — `getList`/`getMany` rethrew the
 *  original untouched, while `getOne`/`create`/`update`/`deleteOne`
 *  flattened it through `toRefineError` into a bare `{ message,
 *  statusCode }` object literal that discarded the value's class, `name`,
 *  `cause` and every own property.
 *
 *  A consumer throwing a typed error from a custom `fetch` transport and
 *  classifying it with `instanceof` therefore got correct behaviour on
 *  list screens and silently wrong behaviour on every detail/create/edit
 *  screen. These tests pin BOTH halves of the fix: every method now goes
 *  through `toRefineError`, and `toRefineError` preserves what it was
 *  given.
 *
 *  The error is raised from the injected `fetch` — before the request
 *  ever leaves the client — which is exactly the shape the report
 *  describes (a `DeviceNotEnrolledError` raised by a custom transport).
 */

class DeviceNotEnrolledError extends Error {
  readonly retryable = false;
  constructor(readonly deviceId: string) {
    super("device is not enrolled");
    this.name = "DeviceNotEnrolledError";
  }
}

/** A transport that fails the way a consumer's own `fetch` wrapper does:
 *  by throwing its own typed error, never reaching the network. */
const refusingFetch: typeof fetch = async () => {
  throw new DeviceNotEnrolledError("device-42");
};

function restProvider() {
  const client = new RefineFixtureClientClient("https://example.test", {
    basePath: "/api",
    fetch: refusingFetch,
  });
  return createCratestackDataProvider(
    { widgets: { api: client.widgets, primaryKey: "id", paged: false } },
    { procedures: { boom: async () => Promise.reject(new DeviceNotEnrolledError("device-42")) } },
  );
}

function rpcProvider() {
  const client = new RefineFixtureRpcClientClient("https://example.test", {
    basePath: "/api",
    fetch: refusingFetch,
  });
  return createCratestackRpcDataProvider(
    { widgets: { api: client.widgets, primaryKey: "id", paged: false } },
    { procedures: { boom: async () => Promise.reject(new DeviceNotEnrolledError("device-42")) } },
  );
}

/** Every `DataProvider` method that awaits a call which can throw, as
 *  `[name, invoke]`. `createMany`/`updateMany`/`deleteMany` delegate to
 *  `create`/`update`/`deleteOne`, so they are covered transitively. */
function methodsOf(provider: ReturnType<typeof restProvider>) {
  return [
    ["getList", () => provider.getList({ resource: "widgets" })],
    ["getMany", () => provider.getMany!({ resource: "widgets", ids: [1] })],
    ["getOne", () => provider.getOne({ resource: "widgets", id: 1 })],
    ["create", () => provider.create({ resource: "widgets", variables: { id: 1, name: "x" } })],
    ["update", () => provider.update({ resource: "widgets", id: 1, variables: { name: "y" } })],
    ["deleteOne", () => provider.deleteOne({ resource: "widgets", id: 1 })],
    ["custom", () => provider.custom!({ url: "", method: "post", meta: { procedure: "boom" } })],
  ] as const;
}

describe("thrown-error preservation across every data-provider method (cratestack#786)", () => {
  for (const [label, makeProvider] of [
    ["REST", restProvider],
    ["RPC", rpcProvider],
  ] as const) {
    describe(label, () => {
      for (const [name, invoke] of methodsOf(makeProvider())) {
        it(`${name} preserves the thrown error's class, so instanceof still classifies it`, async () => {
          const error = await invoke().then(
            () => {
              throw new Error(`${name} should have rejected`);
            },
            (thrown: unknown) => thrown,
          );

          // The half that was silently broken on 4 of 6 methods.
          expect(error).toBeInstanceOf(DeviceNotEnrolledError);
          expect((error as DeviceNotEnrolledError).deviceId).toBe("device-42");
          expect((error as DeviceNotEnrolledError).retryable).toBe(false);
          expect((error as Error).name).toBe("DeviceNotEnrolledError");

          // ...and the half that must not regress: refine reads these two
          // fields off whatever a data provider rejects with.
          expect((error as { statusCode?: unknown }).statusCode).toBe(500);
          expect((error as Error).message).toBe("device is not enrolled");
        });
      }
    });
  }
});

describe("toRefineError", () => {
  it("returns the thrown object itself, annotated — not a copy", () => {
    const thrown = new DeviceNotEnrolledError("device-7");
    expect(toRefineError(thrown)).toBe(thrown);
  });

  it("attaches the original as `cause` when it cannot be annotated in place", () => {
    // A frozen error cannot take `statusCode`; falling back to a plain
    // object with `cause` is the report's stated minimum.
    const frozen = Object.freeze(new DeviceNotEnrolledError("device-9"));
    const converted = toRefineError(frozen);

    expect(converted).not.toBe(frozen);
    expect(converted.statusCode).toBe(500);
    expect(converted.message).toBe("device is not enrolled");
    expect((converted as { cause?: unknown }).cause).toBe(frozen);
  });

  it("attaches a thrown non-object as `cause` rather than losing it", () => {
    const converted = toRefineError("just a string");

    expect(converted.statusCode).toBe(500);
    expect(converted.message).toBe("Unknown error");
    expect((converted as { cause?: unknown }).cause).toBe("just a string");
  });

  it("still promotes a CratestackHttpError's envelope message and status", () => {
    const httpError = Object.assign(new Error("HTTP 422"), {
      status: 422,
      payload: { message: "name must not be empty" },
    });
    const converted = toRefineError(httpError);

    expect(converted.statusCode).toBe(422);
    expect(converted.message).toBe("name must not be empty");
    // Preserved in place, so the raw envelope stays readable.
    expect((converted as { payload?: unknown }).payload).toEqual({
      message: "name must not be empty",
    });
    expect((converted as { status?: unknown }).status).toBe(422);
  });

  it("still surfaces a 412 as the distinguishable optimistic-locking conflict", () => {
    const converted = toRefineError(Object.assign(new Error("HTTP 412"), { status: 412 }));

    expect(converted.statusCode).toBe(412);
    expect(converted.message).toMatch(/changed since it was loaded/);
  });
});
