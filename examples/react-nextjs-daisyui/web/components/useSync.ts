'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import type { LocalClient } from './useLocalNotes';
import type { NoteView } from './protocol';

// Offline-first sync engine.
//
// Source of truth for the user-facing notes list is the wasm/OPFS store.
// Writes happen there first (instant, works offline). A background sync
// pushes everything that's been touched since the last cursor up to the
// napi-backed `/api/notes/sync` endpoint and merges the server's reply
// back into the local store.
//
// State machine:
//   - cursor (RFC3339 timestamp, '' on first run)
//   - online: navigator.onLine, refreshed on online/offline events
//   - pending count: rows the wasm side reports as "newer than cursor"
//
// Auto-sync triggers:
//   - first mount once a client is ready and we're online (one-shot)
//   - online event firing (offline → online transition)
//   - explicit user button
//   - 30s interval (only while tab is visible & online)
//
// Conflict resolution: last-write-wins by updatedAt. Both sides perform
// the same logic so a "newer local" is preserved and re-pushed next round.
//
// Why the ref dance: `push` legitimately needs to read the latest
// `cursor` / `client`, so if it were a useCallback keyed on those, every
// sync would change its identity. Any useEffect that listed `push` in
// its deps would then refire after each sync — calling push again, in
// an infinite loop. We keep a single stable `push` and feed it the
// latest state via refs.

const SYNC_INTERVAL_MS = 30_000;
const CURSOR_KEY = 'cratestack:sync:cursor';

export type SyncState = {
  online: boolean;
  syncing: boolean;
  lastSyncAt: string | null;
  pending: number;
  cursor: string;
  error: string | null;
  push: () => Promise<void>;
};

export function useSync(client: LocalClient | null): SyncState {
  // Default to `true` on both server and client first render — reading
  // `navigator.onLine` here would risk a hydration mismatch if the
  // browser flips that flag between SSR and hydration. The post-mount
  // effect below pulls the real value and wires up listeners.
  const [online, setOnline] = useState(true);
  const [syncing, setSyncing] = useState(false);
  const [lastSyncAt, setLastSyncAt] = useState<string | null>(null);
  const [pending, setPending] = useState(0);
  const [cursor, setCursor] = useState<string>('');
  const [error, setError] = useState<string | null>(null);

  const inFlightRef = useRef(false);
  const clientRef = useRef<LocalClient | null>(client);
  const cursorRef = useRef<string>('');
  clientRef.current = client;
  cursorRef.current = cursor;

  // Hydrate online + cursor from the browser environment after mount.
  useEffect(() => {
    if (typeof navigator !== 'undefined') {
      setOnline(navigator.onLine);
    }
    if (typeof window !== 'undefined') {
      const stored = window.localStorage.getItem(CURSOR_KEY) ?? '';
      if (stored) setCursor(stored);
    }
  }, []);

  // Track navigator online state.
  useEffect(() => {
    if (typeof window === 'undefined') return;
    const handler = () => setOnline(navigator.onLine);
    window.addEventListener('online', handler);
    window.addEventListener('offline', handler);
    return () => {
      window.removeEventListener('online', handler);
      window.removeEventListener('offline', handler);
    };
  }, []);

  // Refresh the pending count off the wasm side whenever cursor/client move.
  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    client
      .since(cursor)
      .then((rows) => {
        if (!cancelled) setPending(rows.length);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [client, cursor, lastSyncAt]);

  // Stable push: empty deps, reads latest client/cursor through refs.
  // Returning the latest identity from useCallback would re-trigger any
  // effect that listed `push` as a dep — and the auto-sync effect below
  // would loop. Anchoring on [] keeps the effect graph quiet.
  const push = useCallback(async (): Promise<void> => {
    const currentClient = clientRef.current;
    if (!currentClient) return;
    if (inFlightRef.current) return;
    if (typeof navigator !== 'undefined' && !navigator.onLine) {
      setError('offline — sync deferred');
      return;
    }
    inFlightRef.current = true;
    setSyncing(true);
    setError(null);
    try {
      const currentCursor = cursorRef.current;
      const pushes = await currentClient.since(currentCursor);
      const response = await fetch('/api/notes/sync', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ cursor: currentCursor, pushes }),
      });
      if (!response.ok) {
        const text = await response.text();
        throw new Error(`sync failed: ${response.status} ${text}`);
      }
      const data = (await response.json()) as {
        cursor: string;
        remote: NoteView[];
      };
      for (const note of data.remote) {
        await currentClient.upsertRemote(note);
      }
      setCursor(data.cursor);
      if (typeof window !== 'undefined') {
        window.localStorage.setItem(CURSOR_KEY, data.cursor);
      }
      setLastSyncAt(new Date().toISOString());
    } catch (raised) {
      setError(raised instanceof Error ? raised.message : String(raised));
    } finally {
      inFlightRef.current = false;
      setSyncing(false);
    }
  }, []);

  // One-shot initial sync once we have a client AND we know we're online.
  // The ref guards against firing again if React re-runs the effect for
  // some other reason (e.g. strict mode double-invoke in development).
  const didInitialSyncRef = useRef(false);
  useEffect(() => {
    if (!client || !online) return;
    if (didInitialSyncRef.current) return;
    didInitialSyncRef.current = true;
    void push();
  }, [client, online, push]);

  // Background sync while tab visible. Effect deps are stable so the
  // interval is set up once per client transition; the body reads the
  // latest push via its closed-over (stable) reference.
  useEffect(() => {
    if (!client) return;
    const interval = setInterval(() => {
      if (typeof document !== 'undefined' && document.visibilityState !== 'visible') return;
      if (typeof navigator !== 'undefined' && !navigator.onLine) return;
      void push();
    }, SYNC_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [client, push]);

  return {
    online,
    syncing,
    lastSyncAt,
    pending,
    cursor,
    error,
    push,
  };
}
