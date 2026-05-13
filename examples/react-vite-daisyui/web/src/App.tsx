import { useCallback, useEffect, useState } from 'react';
import type { FormEvent } from 'react';
import type { NoteView } from './protocol.ts';
import { useNotesWorker } from './useNotesWorker.ts';

export function App() {
  const worker = useNotesWorker();

  return (
    <div className="min-h-screen bg-base-200">
      <header className="navbar bg-base-100 shadow-sm">
        <div className="flex-1">
          <span className="btn btn-ghost text-xl normal-case">
            cratestack · React + Vite + DaisyUI
          </span>
        </div>
        <div className="flex-none gap-2">
          <StorageBadge state={worker} />
        </div>
      </header>

      <main className="container mx-auto max-w-3xl p-4 md:p-6">
        {worker.kind === 'loading' && (
          <div className="flex justify-center py-16">
            <span className="loading loading-spinner loading-lg text-primary" />
          </div>
        )}
        {worker.kind === 'error' && (
          <div role="alert" className="alert alert-error">
            <span>Worker failed: {worker.message}</span>
          </div>
        )}
        {worker.kind === 'ready' && <NotesPanel client={worker.client} />}
      </main>
    </div>
  );
}

function StorageBadge({ state }: { state: ReturnType<typeof useNotesWorker> }) {
  if (state.kind === 'loading') {
    return <span className="badge badge-ghost">booting</span>;
  }
  if (state.kind === 'error') {
    return <span className="badge badge-error">error</span>;
  }
  return state.persistent ? (
    <span className="badge badge-success">OPFS</span>
  ) : (
    <span className="badge badge-warning">in-memory</span>
  );
}

function NotesPanel({ client }: { client: import('./useNotesWorker.ts').NotesClient }) {
  const [notes, setNotes] = useState<NoteView[]>([]);
  const [onlyOpen, setOnlyOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      const rows = await client.list(onlyOpen);
      setNotes(rows);
    } catch (raised) {
      setError(raised instanceof Error ? raised.message : String(raised));
    }
  }, [client, onlyOpen]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

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
    <section className="space-y-6">
      <div className="card bg-base-100 shadow-md">
        <div className="card-body">
          <h2 className="card-title">New note</h2>
          <form onSubmit={onSubmit} className="space-y-3">
            <label className="form-control w-full">
              <div className="label">
                <span className="label-text">Title</span>
              </div>
              <input
                name="title"
                type="text"
                placeholder="Pick up groceries"
                className="input input-bordered w-full"
                required
                maxLength={200}
              />
            </label>
            <label className="form-control w-full">
              <div className="label">
                <span className="label-text">Body</span>
              </div>
              <textarea
                name="body"
                className="textarea textarea-bordered w-full"
                rows={3}
                placeholder="Details, links, anything…"
              />
            </label>
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

      <div className="card bg-base-100 shadow-md">
        <div className="card-body">
          <div className="flex items-center justify-between">
            <h2 className="card-title">
              Notes
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
              No notes yet — add one above. Data persists in OPFS inside this
              browser.
            </p>
          ) : (
            <ul className="space-y-2">
              {notes.map((note) => (
                <NoteRow
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
    </section>
  );
}

function NoteRow({
  note,
  onDone,
  onDelete,
}: {
  note: NoteView;
  onDone: () => Promise<void>;
  onDelete: () => Promise<void>;
}) {
  return (
    <li className="note-card flex items-start gap-3 rounded-lg border border-base-300 p-3">
      <div className="flex-1">
        <div className="flex items-center gap-2">
          {note.pinned && <span className="badge badge-warning badge-sm">pinned</span>}
          {note.completed && <span className="badge badge-success badge-sm">done</span>}
          <h3 className={note.completed ? 'font-semibold line-through opacity-70' : 'font-semibold'}>
            {note.title}
          </h3>
        </div>
        {note.body && (
          <p className="mt-1 text-sm text-base-content/80 whitespace-pre-wrap">
            {note.body}
          </p>
        )}
        <p className="mt-1 text-xs text-base-content/50">
          {new Date(note.createdAt).toLocaleString()}
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
