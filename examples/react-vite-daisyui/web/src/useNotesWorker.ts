// Small React hook wrapping the wasm Worker as a typed async client.
//
// The worker runs on its own thread and answers each `WorkerRequest` with
// a `ReplyMessage` keyed by the same numeric id. We maintain a single map
// of pending promises and resolve/reject them as replies come in. Worker
// startup yields a `ReadyMessage` whose `persistent` field tells us whether
// OPFS succeeded — surfaced through the hook so the UI can warn the user.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type {
  NoteView,
  Operation,
  WorkerResponse,
} from './protocol.ts';

type Pending = {
  resolve: (value: NoteView | NoteView[]) => void;
  reject: (reason: Error) => void;
};

export type NotesClient = {
  add: (input: { title: string; body: string; pinned: boolean }) => Promise<NoteView>;
  list: (onlyOpen: boolean) => Promise<NoteView[]>;
  markDone: (noteId: string) => Promise<NoteView>;
  remove: (noteId: string) => Promise<NoteView>;
};

export type WorkerState =
  | { kind: 'loading' }
  | { kind: 'ready'; persistent: boolean; client: NotesClient }
  | { kind: 'error'; message: string };

export function useNotesWorker(): WorkerState {
  const [state, setState] = useState<WorkerState>({ kind: 'loading' });
  const workerRef = useRef<Worker | null>(null);
  const pendingRef = useRef(new Map<number, Pending>());
  const nextIdRef = useRef(1);

  // We send through a stable callback so the client object stays referentially
  // stable across renders — important when the consumer puts it in a useEffect
  // dependency list.
  const send = useCallback(
    <T extends NoteView | NoteView[]>(operation: Operation): Promise<T> => {
      const worker = workerRef.current;
      if (!worker) {
        return Promise.reject(new Error('worker not ready'));
      }
      const id = nextIdRef.current++;
      return new Promise<T>((resolve, reject) => {
        pendingRef.current.set(id, {
          resolve: resolve as Pending['resolve'],
          reject,
        });
        worker.postMessage({ ...operation, id });
      });
    },
    [],
  );

  const client = useMemo<NotesClient>(
    () => ({
      add: (input) => send<NoteView>({ kind: 'add', ...input }),
      list: (onlyOpen) => send<NoteView[]>({ kind: 'list', only_open: onlyOpen }),
      markDone: (noteId) => send<NoteView>({ kind: 'mark_done', noteId }),
      remove: (noteId) => send<NoteView>({ kind: 'delete', noteId }),
    }),
    [send],
  );

  useEffect(() => {
    const worker = new Worker(new URL('./worker.ts', import.meta.url), {
      type: 'module',
    });
    workerRef.current = worker;

    const onMessage = (event: MessageEvent<WorkerResponse>) => {
      const message = event.data;
      if (message.id === 'ready') {
        setState({ kind: 'ready', persistent: message.persistent, client });
        return;
      }
      const pending = pendingRef.current.get(message.id);
      if (!pending) return;
      pendingRef.current.delete(message.id);
      if (message.ok) {
        pending.resolve(message.payload);
      } else {
        pending.reject(new Error(message.error));
      }
    };

    const onError = (event: ErrorEvent) => {
      setState({ kind: 'error', message: event.message });
    };

    worker.addEventListener('message', onMessage);
    worker.addEventListener('error', onError);

    return () => {
      worker.removeEventListener('message', onMessage);
      worker.removeEventListener('error', onError);
      worker.terminate();
      workerRef.current = null;
      pendingRef.current.clear();
    };
  }, [client]);

  return state;
}
