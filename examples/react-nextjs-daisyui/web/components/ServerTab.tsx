'use client';

import { useCallback, useEffect, useState } from 'react';
import type { FormEvent } from 'react';
import type { NoteView } from './protocol';

// "Server (napi)" tab — fetches the canonical server-owned list via the
// `/api/notes` route, which is backed by the napi-rs addon's SQLite file.
//
// This tab is *always* online: writes are POST'd directly and the napi
// addon stores them. Compared to the Local tab, this is what the user
// would see from another device sharing the same server.

export function ServerTab({ revision }: { revision: number }) {
  const [notes, setNotes] = useState<NoteView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setError(null);
      const response = await fetch('/api/notes', { cache: 'no-store' });
      if (!response.ok) {
        throw new Error(`fetch failed: ${response.status}`);
      }
      const data = (await response.json()) as { notes: NoteView[] };
      setNotes(data.notes);
    } catch (raised) {
      setError(raised instanceof Error ? raised.message : String(raised));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh, revision]);

  const onSubmit = useCallback(
    async (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const form = event.currentTarget;
      const data = new FormData(form);
      const title = String(data.get('title') ?? '').trim();
      if (!title) return;
      try {
        const response = await fetch('/api/notes', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({
            title,
            body: String(data.get('body') ?? ''),
            pinned: data.get('pinned') === 'on',
          }),
        });
        if (!response.ok) {
          throw new Error(`save failed: ${response.status}`);
        }
        form.reset();
        await refresh();
      } catch (raised) {
        setError(raised instanceof Error ? raised.message : String(raised));
      }
    },
    [refresh],
  );

  return (
    <div className="space-y-6">
      <div className="card bg-base-100 shadow">
        <div className="card-body">
          <h2 className="card-title">New server-side note</h2>
          <p className="text-sm text-base-content/70">
            Writes go through the Next.js Route Handler into the napi-rs
            addon's SQLite file. Visible to every device that hits this
            server — try opening another tab.
          </p>
          <form onSubmit={onSubmit} className="mt-2 space-y-3">
            <input
              name="title"
              type="text"
              placeholder="Title"
              className="input input-bordered w-full"
              required
              maxLength={200}
            />
            <textarea
              name="body"
              className="textarea textarea-bordered w-full"
              rows={3}
              placeholder="Body"
            />
            <div className="flex items-center justify-between">
              <label className="label cursor-pointer gap-2">
                <input
                  type="checkbox"
                  name="pinned"
                  className="checkbox checkbox-primary"
                />
                <span className="label-text">Pin</span>
              </label>
              <button type="submit" className="btn btn-secondary">
                Save to server
              </button>
            </div>
          </form>
        </div>
      </div>

      {error && (
        <div role="alert" className="alert alert-error">
          <span>{error}</span>
        </div>
      )}

      <div className="card bg-base-100 shadow">
        <div className="card-body">
          <div className="flex items-center justify-between">
            <h2 className="card-title">
              Server notes
              <span className="badge badge-neutral">{notes.length}</span>
            </h2>
            <button
              type="button"
              onClick={() => void refresh()}
              className="btn btn-sm btn-ghost"
              disabled={loading}
            >
              {loading && <span className="loading loading-spinner loading-xs" />}
              Refresh
            </button>
          </div>
          {notes.length === 0 ? (
            <p className="text-base-content/60 italic">
              No server-side notes yet.
            </p>
          ) : (
            <ul className="space-y-2">
              {notes.map((note) => (
                <li
                  key={note.id}
                  className="note-row rounded-lg border border-base-300 p-3"
                >
                  <div className="flex items-center gap-2">
                    {note.pinned && <span className="badge badge-warning badge-sm">pinned</span>}
                    {note.completed && (
                      <span className="badge badge-success badge-sm">done</span>
                    )}
                    <h3 className="font-semibold">{note.title}</h3>
                  </div>
                  {note.body && (
                    <p className="mt-1 whitespace-pre-wrap text-sm text-base-content/80">
                      {note.body}
                    </p>
                  )}
                  <p className="mt-1 text-xs text-base-content/50">
                    updated {new Date(note.updatedAt).toLocaleString()}
                  </p>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
