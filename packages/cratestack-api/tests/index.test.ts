import { createBatchLink, createLoggerLink } from "@cratestack/api";
import { createRpcBaseQuery } from "@cratestack/api/adapter-rtk";
import { rpcQueryOptions } from "@cratestack/api/adapter-tanstack-query";
import { createAxiosRuntime } from "@cratestack/api/runtime-axios";
import { createFetchRuntime } from "@cratestack/api/runtime-fetch";
import { createYupValidatorLink } from "@cratestack/api/validator-yup";
import { createZodValidatorLink } from "@cratestack/api/validator-zod";
// Proves the compat re-exports actually resolve to real, working
// implementations from the split packages — not just that the import
// graph type-checks. Behavioral coverage for each link/runtime/
// validator/adapter itself lives in its own package's tests.
//
// Imports go through the package's own name ("@cratestack/api", and its
// subpaths), not "../src/*.js" — self-referencing a package by its own
// name resolves through the published `exports` map (Node.js's
// self-reference resolution, since Node 12.16 / npm 7), so this suite
// also catches a broken/mistyped `exports` entry in package.json, which
// a source-relative import would silently sidestep. This is why `test`
// depends on `build` in turbo.json: `exports` points at `./dist/*`.
import { describe, expect, it } from "vitest";

describe("@cratestack/api compat re-exports", () => {
  it("root entry point re-exports the original createBatchLink/createLoggerLink", () => {
    expect(typeof createBatchLink).toBe("function");
    expect(typeof createLoggerLink).toBe("function");
  });

  it("named subpaths re-export the split runtime/validator/adapter packages", () => {
    expect(typeof createFetchRuntime).toBe("function");
    expect(typeof createAxiosRuntime).toBe("function");
    expect(typeof createZodValidatorLink).toBe("function");
    expect(typeof createYupValidatorLink).toBe("function");
    expect(typeof rpcQueryOptions).toBe("function");
    expect(typeof createRpcBaseQuery).toBe("function");
  });
});
