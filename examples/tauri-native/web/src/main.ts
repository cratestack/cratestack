// Pure view layer — no wasm, no Worker, no fetch to upstream services.
// Every data operation, local *or* remote, is a `#[tauri::command]` on
// the Rust side, called here via `@tauri-apps/api/core`'s `invoke()`.
//
// This is the canonical "thick desktop client" pattern: trusted Rust
// owns the database file and any outbound HTTP, and the renderer
// handles UI + user input only.

import { invoke } from '@tauri-apps/api/core';
import type { Article, NewNote, Note } from './protocol';

const noteListEl = document.getElementById('note-list') as HTMLUListElement;
const noteFormEl = document.getElementById('note-form') as HTMLFormElement;
const noteErrorEl = document.getElementById('note-error') as HTMLDivElement;
const onlyOpenEl = document.getElementById('only-open') as HTMLInputElement;
const remoteListEl = document.getElementById('remote-list') as HTMLUListElement;
const remoteFormEl = document.getElementById('remote-form') as HTMLFormElement;
const remoteErrorEl = document.getElementById('remote-error') as HTMLDivElement;

function setError(element: HTMLDivElement, message: string | null): void {
  if (!message) {
    element.hidden = true;
    element.textContent = '';
    return;
  }
  element.hidden = false;
  element.textContent = message;
}

function renderNotes(notes: Note[]): void {
  noteListEl.innerHTML = '';
  if (notes.length === 0) {
    const empty = document.createElement('li');
    empty.className = 'empty';
    empty.textContent = 'No notes yet — add one above.';
    noteListEl.appendChild(empty);
    return;
  }
  for (const note of notes) {
    const li = document.createElement('li');
    li.className = 'note';

    const actions = document.createElement('span');
    actions.className = 'actions';
    if (!note.completed) {
      const doneBtn = document.createElement('button');
      doneBtn.type = 'button';
      doneBtn.textContent = 'Done';
      doneBtn.addEventListener('click', () => {
        void markDone(note.id);
      });
      actions.appendChild(doneBtn);
    }
    const delBtn = document.createElement('button');
    delBtn.type = 'button';
    delBtn.textContent = 'Delete';
    delBtn.addEventListener('click', () => {
      void deleteNote(note.id);
    });
    actions.appendChild(delBtn);

    const title = document.createElement('span');
    title.className = 'title';
    if (note.pinned) {
      const tag = document.createElement('span');
      tag.className = 'pinned';
      tag.textContent = 'pinned';
      title.appendChild(tag);
    }
    if (note.completed) {
      const tag = document.createElement('span');
      tag.className = 'done';
      tag.textContent = 'done';
      title.appendChild(tag);
    }
    title.appendChild(document.createTextNode(note.title));

    const meta = document.createElement('div');
    meta.className = 'meta';
    meta.textContent = `updated ${new Date(note.updatedAt).toLocaleString()}`;

    li.appendChild(actions);
    li.appendChild(title);
    li.appendChild(meta);
    noteListEl.appendChild(li);
  }
}

function renderArticles(articles: Article[]): void {
  remoteListEl.innerHTML = '';
  if (articles.length === 0) {
    const empty = document.createElement('li');
    empty.className = 'empty';
    empty.textContent =
      'No articles. Point this at a running CrateStack service exposing the Article model.';
    remoteListEl.appendChild(empty);
    return;
  }
  for (const article of articles) {
    const li = document.createElement('li');
    li.className = 'article';
    const head = document.createElement('div');
    head.innerHTML = `<strong>${escapeHtml(article.title)}</strong> ${
      article.published ? '<span class="done">published</span>' : ''
    }`;
    const meta = document.createElement('div');
    meta.className = 'meta';
    meta.textContent = new Date(article.createdAt).toLocaleString();
    li.appendChild(head);
    if (article.body) {
      const body = document.createElement('div');
      body.textContent = article.body;
      li.appendChild(body);
    }
    li.appendChild(meta);
    remoteListEl.appendChild(li);
  }
}

function escapeHtml(input: string): string {
  return input
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}

async function refreshNotes(): Promise<void> {
  try {
    setError(noteErrorEl, null);
    const notes = await invoke<Note[]>('list_notes', { onlyOpen: onlyOpenEl.checked });
    renderNotes(notes);
  } catch (error) {
    setError(noteErrorEl, error instanceof Error ? error.message : String(error));
  }
}

async function markDone(id: string): Promise<void> {
  try {
    setError(noteErrorEl, null);
    await invoke('mark_done', { id });
    await refreshNotes();
  } catch (error) {
    setError(noteErrorEl, error instanceof Error ? error.message : String(error));
  }
}

async function deleteNote(id: string): Promise<void> {
  try {
    setError(noteErrorEl, null);
    await invoke('delete_note', { id });
    await refreshNotes();
  } catch (error) {
    setError(noteErrorEl, error instanceof Error ? error.message : String(error));
  }
}

noteFormEl.addEventListener('submit', async (event) => {
  event.preventDefault();
  const data = new FormData(noteFormEl);
  const title = String(data.get('title') ?? '').trim();
  if (!title) return;
  const input: NewNote = {
    title,
    body: '',
    pinned: data.get('pinned') === 'on',
  };
  try {
    setError(noteErrorEl, null);
    await invoke<Note>('add_note', { input });
    noteFormEl.reset();
    await refreshNotes();
  } catch (error) {
    setError(noteErrorEl, error instanceof Error ? error.message : String(error));
  }
});

onlyOpenEl.addEventListener('change', () => {
  void refreshNotes();
});

remoteFormEl.addEventListener('submit', async (event) => {
  event.preventDefault();
  const data = new FormData(remoteFormEl);
  const url = String(data.get('url') ?? '').trim();
  if (!url) return;
  try {
    setError(remoteErrorEl, null);
    const articles = await invoke<Article[]>('fetch_remote_articles', { baseUrl: url });
    renderArticles(articles);
  } catch (error) {
    setError(remoteErrorEl, error instanceof Error ? error.message : String(error));
  }
});

// Initial paint.
void refreshNotes();
