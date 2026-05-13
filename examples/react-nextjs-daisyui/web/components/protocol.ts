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
  | { kind: 'delete'; noteId: string }
  | { kind: 'upsert_remote'; note: NoteView }
  | { kind: 'since'; cursor: string };

export type WorkerRequest = Operation & { id: number };

export type ReplyMessage =
  | { id: number; ok: true; payload: NoteView | NoteView[] }
  | { id: number; ok: false; error: string };

export type ReadyMessage = { id: 'ready'; ok: true; persistent: boolean };

export type WorkerResponse = ReplyMessage | ReadyMessage;
