/// <reference lib="webworker" />
//
// Dedicated Worker hosting the wasm-compiled rusqlite runtime. Identical
// shape to the vanilla TS example — only the JS consumer changes.

import init, {
  init_panic_hook,
  install_opfs,
  open_db,
  open_in_memory,
  add_note,
  list_notes,
  mark_done,
  delete_note,
} from '../pkg/react_vite_daisyui_example.js';
import type { WorkerRequest, WorkerResponse } from './protocol.ts';

const ctx = self as DedicatedWorkerGlobalScope;

let persistent = false;

async function bootstrap(): Promise<void> {
  await init();
  init_panic_hook();
  try {
    await install_opfs();
    open_db('notes.db');
    persistent = true;
  } catch (error) {
    console.warn(
      '[cratestack worker] OPFS unavailable, falling back to in-memory storage:',
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
        const created = add_note({
          title: req.title,
          body: req.body,
          pinned: req.pinned,
        });
        reply({ id: req.id, ok: true, payload: created });
        break;
      }
      case 'list': {
        const rows = list_notes(req.only_open);
        reply({ id: req.id, ok: true, payload: rows });
        break;
      }
      case 'mark_done': {
        const updated = mark_done(req.noteId);
        reply({ id: req.id, ok: true, payload: updated });
        break;
      }
      case 'delete': {
        const removed = delete_note(req.noteId);
        reply({ id: req.id, ok: true, payload: removed });
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
