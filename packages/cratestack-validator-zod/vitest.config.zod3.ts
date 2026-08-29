import { defineConfig } from "vitest/config";

// Second test run for the zod-3 half of this package's PUBLISHED peer
// range, `zod: "^3.23.0 || ^4.0.0"`.
//
// The default run (`vitest.config.ts`-less, plain `vitest run`) resolves
// `zod` to the `^4.0.0` devDependency, so before this config existed the
// `^3.23.0` half of that promise had nothing behind it — a zod-3-only
// break would have shipped green. Rather than narrow the published range
// (consumers on zod 3 are real), this aliases the module specifier to the
// `zod3` devDependency (`npm:zod@^3.23.0`) and re-runs the *same* suite,
// so both advertised majors are exercised from one set of test files.
//
// Note this only covers runtime behaviour. The type-level half of the
// same promise — `ZodTypeAny`, which zod 4 deprecates and a "modernizing"
// edit could easily replace with a zod-4-only spelling — is covered
// separately by `tsconfig.zod3.json`, since vitest transpiles without
// type-checking. Both run from the `test` script.
export default defineConfig({
  resolve: {
    alias: {
      zod: "zod3",
    },
  },
  test: {
    // Fails the run loudly if the alias above ever stops taking effect —
    // see the file's own comment. A silently-unaliased run would print a
    // green `3 passed` while testing zod 4 twice.
    setupFiles: ["./tests/support/assert-zod3.ts"],
  },
});
