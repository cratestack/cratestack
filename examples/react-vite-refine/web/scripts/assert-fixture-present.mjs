#!/usr/bin/env node
// Fails loudly, not silently, when `generated/` (the `cratestack
// generate-typescript --refine` output) is missing — mirrors
// `packages/cratestack-refine/tests/support/assert-fixture-present.ts`'s
// vitest `globalSetup` guard for the same "gitignored, must be generated
// first" situation (cratestack#577: don't commit generated build output).
// Without this, a missing `generated/` surfaces as an opaque Vite/tsc
// "Cannot find module './generated/src/client.js'" instead of pointing at
// the one command that fixes it.
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

const marker = fileURLToPath(new URL("../generated/src/client.ts", import.meta.url));

if (!existsSync(marker)) {
  console.error(
    "\ngenerated/ is missing (gitignored — it's build output, not committed).\n" +
      "Run this from the repo root first:\n\n" +
      "  just react-vite-refine-fixture\n\n" +
      "then re-run this command.\n",
  );
  process.exit(1);
}
