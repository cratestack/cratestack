'use client';

import { useCallback, useEffect, useState } from 'react';
import type { FormEvent } from 'react';
import type { LocalClient } from './useLocalNotes';
import type { NoteView } from './protocol';

// "Local (wasm)" tab — writes hit the OPFS-backed SQLite first. The sync
// engine pushes them up to the napi store in the background. Refreshes
// after each mutation AND whenever `revision` ticks (the parent bumps it
// after a sync round-trip so we re-pull server-authored merges).

export function LocalTab({
  client,
  revision,
}: {
  client: LocalClient;
  revision: number;
}) {
  const [notes, setNotes] = useState<NoteView[]>([]);
  const [onlyOpen, setOnlyOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      setNotes(await client.list(onlyOpen));
    } catch (raised) {
      setError(raised instanceof Error ? raised.message : String(raised));
    }
  }, [client, onlyOpen]);

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
      setPending(true);
      try {
        await client.add({
          title,
          body: String(data.get('body') ?? ''),
          pinned: data.get('pinned') === 'on',
        });
        form.reset();
        await refresh();
      } catch (raised) {
        setError(raised instanceof Error ? raised.message : String(raised));
      } finally {
        setPending(false);
      }
    },
    [client, refresh],
  );

  return (
    <div className="space-y-6">
      <div className="card bg-base-100 shadow">
        <div className="card-body">
          <h2 className="card-title">New local note</h2>
          <p className="text-sm text-base-content/70">
            Stored in OPFS inside this browser. Writes show up immediately
            and are pushed to the server on the next sync.
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
                <span className="label-text">Pin to top</span>
              </label>
              <button
                type="submit"
                className="btn btn-primary"
                disabled={pending}
              >
                {pending && <span className="loading loading-spinner loading-xs" />}
                Save
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
              Local notes
              <span className="badge badge-neutral">{notes.length}</span>
            </h2>
            <label className="label cursor-pointer gap-2">
              <span className="label-text">Hide completed</span>
              <input
                type="checkbox"
                className="toggle toggle-primary"
                checked={onlyOpen}
                onChange={(event) => setOnlyOpen(event.target.checked)}
              />
            </label>
          </div>
          {notes.length === 0 ? (
            <p className="text-base-content/60 italic">
              No notes yet — add one above.
            </p>
          ) : (
            <ul className="space-y-2">
              {notes.map((note) => (
                <LocalRow
                  key={note.id}
                  note={note}
                  onDone={async () => {
                    await client.markDone(note.id);
                    await refresh();
                  }}
                  onDelete={async () => {
                    await client.remove(note.id);
                    await refresh();
                  }}
                />
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}

function LocalRow({
  note,
  onDone,
  onDelete,
}: {
  note: NoteView;
  onDone: () => Promise<void>;
  onDelete: () => Promise<void>;
}) {
  return (
    <li className="note-row flex items-start gap-3 rounded-lg border border-base-300 p-3">
      <div className="flex-1">
        <div className="flex items-center gap-2">
          {note.pinned && <span className="badge badge-warning badge-sm">pinned</span>}
          {note.completed && <span className="badge badge-success badge-sm">done</span>}
          <h3 className={note.completed ? 'font-semibold line-through opacity-70' : 'font-semibold'}>
            {note.title}
          </h3>
        </div>
        {note.body && (
          <p className="mt-1 whitespace-pre-wrap text-sm text-base-content/80">
            {note.body}
          </p>
        )}
        <p className="mt-1 text-xs text-base-content/50">
          updated {new Date(note.updatedAt).toLocaleString()}
        </p>
      </div>
      <div className="join">
        {!note.completed && (
          <button
            type="button"
            className="btn btn-sm btn-ghost join-item"
            onClick={() => void onDone()}
          >
            Done
          </button>
        )}
        <button
          type="button"
          className="btn btn-sm btn-ghost text-error join-item"
          onClick={() => void onDelete()}
        >
          Delete
        </button>
      </div>
    </li>
  );
}
