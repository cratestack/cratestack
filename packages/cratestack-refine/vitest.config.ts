import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // See tests/support/assert-fixture-present.ts's own doc comment:
    // fails the whole run loudly (not a silent skip) when the real
    // generated TypeScript client fixture this package's tests drive
    // hasn't been produced yet (`just refine-fixture`).
    globalSetup: ["./tests/support/assert-fixture-present.ts"],
  },
});
