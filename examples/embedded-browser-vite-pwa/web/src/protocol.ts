// Wire-format types shared by the main thread and the worker.
//
// The worker hosts the rusqlite `RusqliteRuntime`; the main thread issues
// requests over `postMessage` and waits for matching responses. Requests
// carry an `id` so concurrent calls don't race.

export interface NoteView {
  id: string;
  title: string;
  body: string;
  pinned: boolean;
  completed: boolean;
  createdAt: string;
  updatedAt: string;
}

export type Operation =
  | { kind: 'add'; title: string; body: string; pinned: boolean }
  | { kind: 'list'; only_open: boolean }
  | { kind: 'mark_done'; noteId: string }
  | { kind: 'delete'; noteId: string };

export type WorkerRequest = Operation & { id: number };

export type ReplyMessage =
  | { id: number; ok: true; payload: NoteView }
  | { id: number; ok: true; payload: NoteView[] }
  | { id: number; ok: false; error: string };

export type ReadyMessage = { id: 'ready'; ok: true; persistent: boolean };

export type WorkerResponse = ReplyMessage | ReadyMessage;
