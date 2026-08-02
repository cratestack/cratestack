// Issue #306 AC #4: calling the generated plain functions completely
// outside React — a script, not a page load. Every import below comes
// straight from the generated package's plain-function source files, no
// `.hooks` subpath, no `swr`/React installed — proving the functions in
// `client/src/models/*.ts` and `client/src/procedures.ts` really are
// framework-free, the same claim `tests/swr_runtime.rs` makes on the
// Rust side of the generator.
//
// Run via `tsx` (a plain TS runner, not a UI framework), matching that
// same Rust test's own precedent, against a live server
// (`cargo run -p react-vite-swr-example`):
//   pnpm run seed
//
// Imports below are relative source paths into `../client/src`, not the
// installed `react-vite-swr-client` package name, for a real, if
// unglamorous, reason: the generator's compiled `client/dist/` output
// currently can't be `import`ed by plain Node at all — every relative
// import in every generated file (`./runtime`, `../queries`, ...) is
// missing its `.js` extension, which Node's ESM resolver requires
// outside a bundler. `tsx`'s bundler-style resolution tolerates it for
// TS *source* files (exactly why `tests/swr_runtime.rs` also imports
// generated source, never `dist/`), but does nothing for the
// already-compiled `.js` this script would otherwise load through
// `node_modules`. Filed as https://github.com/cratestack/cratestack/issues/315
// rather than silently worked around — fixing it properly means adding
// `.js` extensions across ~20 template files (both presets, REST/RPC),
// out of scope for this example. This script demonstrates the plain-
// function-outside-React claim for real; it does not demonstrate the
// installed-package path, which is what #315 tracks.
import {
  CratestackRuntime,
  createBoard,
  createTask,
  estimateFocusMinutes,
  listBoards,
} from "../../client/src/index.ts";

const runtime = new CratestackRuntime("http://127.0.0.1:3210", {
  basePath: "/api",
  headers: { "x-auth-id": "1" },
});

async function main() {
  const boards = await listBoards(runtime);
  console.log(`[seed] found ${boards.length} existing board(s)`);

  let board = boards.find((candidate) => candidate.name === "Getting Started");
  if (!board) {
    board = await createBoard(runtime, { id: Date.now(), name: "Getting Started" });
    console.log("[seed] created board:", board);

    const seedTasks = ["Write the schema", "Generate the client", "Run the app"];
    for (const [index, title] of seedTasks.entries()) {
      const task = await createTask(runtime, {
        id: Date.now() + index,
        title,
        done: false,
        boardId: board.id,
      });
      console.log("[seed] created task:", task);
    }
  } else {
    console.log("[seed] 'Getting Started' board already exists — skipping task seed");
  }

  // The procedure's plain function, same two-layer shape as the models:
  // no hook, no `swr` import, just an `async function` you can call from
  // anywhere.
  const estimate = await estimateFocusMinutes(runtime, {
    args: { taskCount: 3, minutesPerTask: 25 },
  });
  console.log(`[seed] estimateFocusMinutes(3 tasks) -> ${estimate.totalMinutes} minutes`);
}

main().catch((error) => {
  console.error("[seed] FAILED:", error);
  process.exitCode = 1;
});
