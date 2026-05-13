'use client';

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import type { NoteView, Operation, WorkerResponse } from './protocol';

type Pending = {
  resolve: (value: NoteView | NoteView[]) => void;
  reject: (reason: Error) => void;
};

export type LocalClient = {
  add: (input: { title: string; body: string; pinned: boolean }) => Promise<NoteView>;
  list: (onlyOpen: boolean) => Promise<NoteView[]>;
  markDone: (noteId: string) => Promise<NoteView>;
  remove: (noteId: string) => Promise<NoteView>;
  upsertRemote: (note: NoteView) => Promise<NoteView>;
  since: (cursor: string) => Promise<NoteView[]>;
};

export type LocalState =
  | { kind: 'loading' }
  | { kind: 'ready'; persistent: boolean; client: LocalClient }
  | { kind: 'error'; message: string };

export function useLocalNotes(): LocalState {
  const [state, setState] = useState<LocalState>({ kind: 'loading' });
  const workerRef = useRef<Worker | null>(null);
  const pendingRef = useRef(new Map<number, Pending>());
  const nextIdRef = useRef(1);

  const send = useCallback(
    <T extends NoteView | NoteView[]>(operation: Operation): Promise<T> => {
      const worker = workerRef.current;
      if (!worker) return Promise.reject(new Error('worker not ready'));
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

  const client = useMemo<LocalClient>(
    () => ({
      add: (input) => send<NoteView>({ kind: 'add', ...input }),
      list: (onlyOpen) => send<NoteView[]>({ kind: 'list', only_open: onlyOpen }),
      markDone: (noteId) => send<NoteView>({ kind: 'mark_done', noteId }),
      remove: (noteId) => send<NoteView>({ kind: 'delete', noteId }),
      upsertRemote: (note) => send<NoteView>({ kind: 'upsert_remote', note }),
      since: (cursor) => send<NoteView[]>({ kind: 'since', cursor }),
    }),
    [send],
  );

  useEffect(() => {
    // Only run on the client — guard against any chance of SSR import.
    if (typeof window === 'undefined') return;
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
      if (message.ok) pending.resolve(message.payload);
      else pending.reject(new Error(message.error));
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
