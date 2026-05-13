// Typed messages between the main thread and the wasm Dedicated Worker.
//
// `Operation` is the union of inbound RPCs (no `id` yet). `WorkerRequest`
// pairs each operation with a correlation id so the main thread can match
// replies. Replies fall into `ReplyMessage` (normal completion) or
// `ReadyMessage` (initial bootstrap signal). Splitting them keeps the
// discriminated-union narrowing tidy on the consumer side.

export type NoteView = {
  id: string;
  title: string;
  body: string;
  pinned: boolean;
  completed: boolean;
  createdAt: string;
  updatedAt: string;
};

export type Operation =
  | { kind: 'add'; title: string; body: string; pinned: boolean }
  | { kind: 'list'; only_open: boolean }
  | { kind: 'mark_done'; noteId: string }
  | { kind: 'delete'; noteId: string };

export type WorkerRequest = Operation & { id: number };

export type ReplyMessage =
  | { id: number; ok: true; payload: NoteView | NoteView[] }
  | { id: number; ok: false; error: string };

export type ReadyMessage = { id: 'ready'; ok: true; persistent: boolean };

export type WorkerResponse = ReplyMessage | ReadyMessage;
