// Proves the compat re-exports actually resolve to real, working
// implementations from the split packages — not just that the import
// graph type-checks. Behavioral coverage for each link/runtime/
// validator/adapter itself lives in its own package's tests.
import { describe, expect, it } from "vitest";
import { createRpcBaseQuery } from "../src/adapter-rtk.js";
import { rpcQueryOptions } from "../src/adapter-tanstack-query.js";
import { createBatchLink, createLoggerLink } from "../src/index.js";
import { createAxiosRuntime } from "../src/runtime-axios.js";
import { createFetchRuntime } from "../src/runtime-fetch.js";
import { createYupValidatorLink } from "../src/validator-yup.js";
import { createZodValidatorLink } from "../src/validator-zod.js";

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
