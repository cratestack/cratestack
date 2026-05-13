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
//   - mount (if online)
//   - online event firing
//   - explicit user button
//   - 30s interval (only while tab is visible & online)
//
// Conflict resolution: last-write-wins by updatedAt. Both sides perform
// the same logic so a "newer local" is preserved and re-pushed next round.

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
  const [online, setOnline] = useState(
    typeof navigator === 'undefined' ? true : navigator.onLine,
  );
  const [syncing, setSyncing] = useState(false);
  const [lastSyncAt, setLastSyncAt] = useState<string | null>(null);
  const [pending, setPending] = useState(0);
  const [cursor, setCursor] = useState<string>('');
  const [error, setError] = useState<string | null>(null);

  const inFlightRef = useRef(false);

  // Restore the cursor from localStorage on first mount; means a refresh
  // doesn't re-pull the entire server-side history.
  useEffect(() => {
    if (typeof window === 'undefined') return;
    const stored = window.localStorage.getItem(CURSOR_KEY) ?? '';
    setCursor(stored);
  }, []);

  // Track navigator online state so the UI can show offline banners.
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

  const push = useCallback(async (): Promise<void> => {
    if (!client) return;
    if (inFlightRef.current) return;
    if (typeof navigator !== 'undefined' && !navigator.onLine) {
      setError('offline — sync deferred');
      return;
    }
    inFlightRef.current = true;
    setSyncing(true);
    setError(null);
    try {
      const pushes = await client.since(cursor);
      const response = await fetch('/api/notes/sync', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ cursor, pushes }),
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
        await client.upsertRemote(note);
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
  }, [client, cursor]);

  // Trigger an initial sync once the worker is up and we're online.
  useEffect(() => {
    if (!client || !online) return;
    void push();
  }, [client, online, push]);

  // Background sync while tab visible.
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
