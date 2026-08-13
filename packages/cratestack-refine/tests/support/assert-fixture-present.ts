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
 *  and every cratestack-pg gRPC test both silently reported `ok` under
 *  exactly that failure shape for an extended period before being
 *  caught. A test suite that can pass with zero real assertions is
 *  worse than one that fails loudly and tells you why. */
export default function assertFixturePresent(): void {
  const here = dirname(fileURLToPath(import.meta.url));
  const marker = join(here, "..", "fixtures", "generated-client", "src", "client.ts");

  if (!existsSync(marker)) {
    throw new Error(
      "\n\n@cratestack/refine's test suite drives a REAL generated TypeScript client " +
        "fixture that is not present.\n\n" +
        "Generate it first:\n\n" +
        "    just refine-fixture\n\n" +
        `(expected to find: ${marker})\n\n` +
        "This is intentional, not a broken checkout: cratestack#577 — generated build " +
        "output is never committed to this repo. See justfile's refine-fixture recipe " +
        'and .github/workflows/ci.yml\'s "js (@cratestack/refine)" job.\n',
    );
  }
}
