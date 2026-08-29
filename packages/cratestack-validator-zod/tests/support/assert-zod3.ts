import { beforeAll } from "vitest";
import zodPkg from "zod/package.json" with { type: "json" };

// Guard for the zod-3 run (`vitest.config.zod3.ts`), which exercises the
// lower half of this package's published peer range by aliasing the `zod`
// specifier to the `zod3` devDependency.
//
// Without this, the alias is a silent single point of failure: drop it,
// rename the `zod3` dependency, or let vite stop applying `resolve.alias`
// to `tests/`, and the second run quietly re-tests zod 4 for a second time
// and still prints a green `3 passed`. That is the exact failure mode the
// second run exists to prevent, so it is asserted rather than assumed —
// same discipline as `@cratestack/refine`'s `assert-fixture-present.ts`.
beforeAll(() => {
  if (!zodPkg.version.startsWith("3.")) {
    throw new Error(
      `zod-3 run resolved zod@${zodPkg.version}, not a 3.x. The \`zod\` -> \`zod3\` ` +
        "alias in vitest.config.zod3.ts is not taking effect, so this run is " +
        "re-testing the same major as the default run and proves nothing about " +
        "the `^3.23.0` half of the published peer range.",
    );
  }
});
