// Main-thread UI for the tauri-web example.
//
// Two surfaces wired up here:
//
//   1. Local notes — spawned worker hosts the wasm `cratestack-rusqlite`
//      runtime, persists data via OPFS, and exchanges typed messages.
//   2. Remote articles — Tauri command `fetch_remote_articles` invoked via
//      `@tauri-apps/api/core`. The native Rust side owns the HTTP client
//      (include_client_schema!) so the webview never makes a fetch itself.

import { invoke } from '@tauri-apps/api/core';
import type {
  NoteView,
  Operation,
  ReplyMessage,
  WorkerRequest,
  WorkerResponse,
} from './protocol.ts';

const statusEl = document.getElementById('status') as HTMLDivElement;
const listEl = document.getElementById('list') as HTMLUListElement;
const form = document.getElementById('add') as HTMLFormElement;
const titleInput = document.getElementById('title') as HTMLInputElement;
const bodyInput = document.getElementById('body') as HTMLInputElement;
const pinnedInput = document.getElementById('pinned') as HTMLInputElement;

const remoteForm = document.getElementById('remote') as HTMLFormElement;
const remoteUrlInput = document.getElementById('remote-url') as HTMLInputElement;
const remoteStatusEl = document.getElementById('remote-status') as HTMLDivElement;
const remoteListEl = document.getElementById('remote-list') as HTMLUListElement;

interface ArticleView {
  id: number;
  title: string;
  published: boolean;
}

const worker = new Worker(new URL('./worker.ts', import.meta.url), {
  type: 'module',
});

let nextId = 1;
const pending = new Map<number, (response: ReplyMessage) => void>();
const ready: Promise<{ persistent: boolean }> = new Promise((resolve, reject) => {
  worker.addEventListener('message', (event: MessageEvent<WorkerResponse>) => {
    const message = event.data;
    if (message.id === 'ready') {
      if (message.ok) {
        resolve({ persistent: message.persistent });
      } else {
        reject(new Error('worker failed to start'));
      }
      return;
    }
    const resolver = pending.get(message.id);
    if (resolver) {
      pending.delete(message.id);
      resolver(message);
    }
  });
});

async function call<T>(operation: Operation): Promise<T> {
  await ready;
  const id = nextId++;
  return new Promise<T>((resolve, reject) => {
    pending.set(id, (response) => {
      if (response.ok) {
        resolve(response.payload as T);
      } else {
        reject(new Error(response.error));
      }
    });
    worker.postMessage({ id, ...operation } as WorkerRequest);
  });
}

function renderList(rows: NoteView[]): void {
  if (rows.length === 0) {
    listEl.innerHTML = '<li><em>(no notes yet)</em></li>';
    return;
  }
  listEl.innerHTML = '';
  for (const note of rows) {
    const li = document.createElement('li');
    if (note.completed) li.classList.add('done');
    const pin = document.createElement('span');
    pin.className = 'pin';
    pin.textContent = note.pinned ? '📌' : '  ';
    const title = document.createElement('span');
    title.className = 'title';
    title.textContent = note.title;
    const body = document.createElement('span');
    body.className = 'body';
    body.textContent = note.body;
    const actions = document.createElement('span');
    actions.className = 'actions';
    if (!note.completed) {
      const doneBtn = document.createElement('button');
      doneBtn.textContent = 'done';
      doneBtn.addEventListener('click', () => onMarkDone(note.id));
      actions.append(doneBtn);
    }
    const deleteBtn = document.createElement('button');
    deleteBtn.textContent = 'delete';
    deleteBtn.addEventListener('click', () => onDelete(note.id));
    actions.append(deleteBtn);

    li.append(pin, title, body, actions);
    listEl.append(li);
  }
}

async function refresh(): Promise<void> {
  try {
    const rows = await call<NoteView[]>({ kind: 'list', only_open: false });
    renderList(rows);
  } catch (error) {
    statusEl.textContent = `list failed: ${(error as Error).message}`;
  }
}

async function onMarkDone(noteId: string): Promise<void> {
  await call<NoteView>({ kind: 'mark_done', noteId });
  void refresh();
}

async function onDelete(noteId: string): Promise<void> {
  await call<NoteView>({ kind: 'delete', noteId });
  void refresh();
}

form.addEventListener('submit', async (event) => {
  event.preventDefault();
  const title = titleInput.value.trim();
  if (!title) return;
  await call<NoteView>({
    kind: 'add',
    title,
    body: bodyInput.value,
    pinned: pinnedInput.checked,
  });
  titleInput.value = '';
  bodyInput.value = '';
  pinnedInput.checked = false;
  void refresh();
});

function renderRemote(rows: ArticleView[]): void {
  if (rows.length === 0) {
    remoteListEl.innerHTML = '<li><em>(no articles returned)</em></li>';
    return;
  }
  remoteListEl.innerHTML = '';
  for (const article of rows) {
    const li = document.createElement('li');
    const marker = document.createElement('span');
    marker.className = 'pin';
    marker.textContent = article.published ? '🗞 ' : '📝 ';
    const title = document.createElement('span');
    title.className = 'title';
    title.textContent = `#${article.id}  ${article.title}`;
    li.append(marker, title);
    remoteListEl.append(li);
  }
}

remoteForm.addEventListener('submit', async (event) => {
  event.preventDefault();
  const baseUrl = remoteUrlInput.value.trim();
  if (!baseUrl) return;
  remoteStatusEl.textContent = `calling Tauri command fetch_remote_articles(${baseUrl})…`;
  try {
    const articles = await invoke<ArticleView[]>('fetch_remote_articles', {
      baseUrl,
    });
    remoteStatusEl.textContent = `✓ received ${articles.length} article(s) via native HTTP client`;
    renderRemote(articles);
  } catch (error) {
    remoteStatusEl.textContent = `✗ remote call failed: ${error}`;
    renderRemote([]);
  }
});

void (async () => {
  try {
    const info = await ready;
    statusEl.textContent = info.persistent
      ? '✓ ready (OPFS-backed, persistent across reloads)'
      : '⚠ ready (in-memory fallback — OPFS unavailable, data lost on reload)';
    await refresh();
  } catch (error) {
    statusEl.textContent = `startup failed: ${(error as Error).message}`;
  }
})();
