/// <reference lib="webworker" />
//
// Dedicated Worker hosting the wasm-compiled rusqlite runtime for the
// "Local (wasm)" tab and the offline-first sync engine.
//
// The wasm bundle lives under `/public/pkg/...` (built by
// `pnpm run wasm:build`). We `init()` it explicitly with a URL pointing
// at the bundled `.wasm` file so the path is stable regardless of which
// page in the App Router hosts the worker.

import init, {
  init_panic_hook,
  install_opfs,
  open_db,
  open_in_memory,
  add_note,
  list_notes,
  mark_done,
  delete_note,
  upsert_remote_note,
  notes_since,
} from '../public/pkg/react_nextjs_daisyui_wasm';
import type { WorkerRequest, WorkerResponse } from './protocol';

const ctx = self as DedicatedWorkerGlobalScope;

let persistent = false;

async function bootstrap(): Promise<void> {
  // Workers under Next can resolve the .wasm asset relative to this script
  // via the public-folder URL. We let wasm-bindgen pick that up by default.
  await init();
  init_panic_hook();
  try {
    await install_opfs();
    open_db('notes.db');
    persistent = true;
  } catch (error) {
    console.warn(
      '[cratestack worker] OPFS unavailable, falling back to in-memory:',
      error,
    );
    open_in_memory();
    persistent = false;
  }
  ctx.postMessage({ id: 'ready', ok: true, persistent } satisfies WorkerResponse);
}

function reply(response: WorkerResponse): void {
  ctx.postMessage(response);
}

function handle(req: WorkerRequest): void {
  try {
    switch (req.kind) {
      case 'add': {
        reply({
          id: req.id,
          ok: true,
          payload: add_note({
            title: req.title,
            body: req.body,
            pinned: req.pinned,
          }),
        });
        break;
      }
      case 'list': {
        reply({ id: req.id, ok: true, payload: list_notes(req.only_open) });
        break;
      }
      case 'mark_done': {
        reply({ id: req.id, ok: true, payload: mark_done(req.noteId) });
        break;
      }
      case 'delete': {
        reply({ id: req.id, ok: true, payload: delete_note(req.noteId) });
        break;
      }
      case 'upsert_remote': {
        reply({
          id: req.id,
          ok: true,
          payload: upsert_remote_note(req.note),
        });
        break;
      }
      case 'since': {
        reply({ id: req.id, ok: true, payload: notes_since(req.cursor) });
        break;
      }
    }
  } catch (error) {
    reply({
      id: req.id,
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

ctx.addEventListener('message', (event: MessageEvent<WorkerRequest>) => {
  handle(event.data);
});

void bootstrap();
