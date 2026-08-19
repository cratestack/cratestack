import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/** vitest `globalSetup` (wired in `vitest.config.ts`), run once before any
 *  test file. This package's tests drive a REAL generated TypeScript
 *  client (cratestack#571) that is deliberately NOT committed to the repo
 *  (cratestack#577 — "don't commit generated build output" is a hard
 *  `CLAUDE.md` rule) — `just refine-fixture` produces it locally, and
 *  CI's `js (@cratestack/refine)` job produces it fresh on a
 *  Rust-toolchain runner before testing.
 *
 *  Throwing here fails the whole vitest run with a non-zero exit code
 *  and this message — it does NOT print a green "0 tests" the way a
 *  missing-import `describe.skip`/`it.skip` fallback would. That
 *  distinction is the entire point: cratestack-studio's Postgres tests
 *  silently reported `ok` under exactly that failure shape for an
 *  extended period before being caught. A test suite that can pass with
 *  zero real assertions is worse than one that fails loudly and tells
 *  you why. */
/** One fixture this check looks for — REST (`refine-fixture`) and RPC
 *  (`refine-rpc-fixture`) are both generated-and-gitignored (cratestack#577)
 *  and both required, since the test suite drives a real generated client
 *  of each transport. */
interface FixtureMarker {
  recipe: string;
  marker: string;
}

export default function assertFixturePresent(): void {
  const here = dirname(fileURLToPath(import.meta.url));
  const fixtures: FixtureMarker[] = [
    {
      recipe: "refine-fixture",
      marker: join(here, "..", "fixtures", "generated-client", "src", "client.ts"),
    },
    {
      recipe: "refine-rpc-fixture",
      marker: join(here, "..", "fixtures", "generated-client-rpc", "src", "client.ts"),
    },
  ];

  const missing = fixtures.filter((fixture) => !existsSync(fixture.marker));
  if (missing.length === 0) return;

  const instructions = missing
    .map((fixture) => `    just ${fixture.recipe}\n    (expected to find: ${fixture.marker})`)
    .join("\n\n");

  throw new Error(
    "\n\n@cratestack/refine's test suite drives REAL generated TypeScript client " +
      "fixtures that are not present.\n\n" +
      "Generate them first:\n\n" +
      `${instructions}\n\n` +
      "This is intentional, not a broken checkout: cratestack#577 — generated build " +
      "output is never committed to this repo. See justfile's refine-fixture/" +
      'refine-rpc-fixture recipes and .github/workflows/ci.yml\'s "js (@cratestack/refine)" ' +
      "job.\n",
  );
}
