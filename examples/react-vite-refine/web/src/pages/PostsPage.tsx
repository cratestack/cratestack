// `@@paged` + `@version`. The refine dataProvider (`@cratestack/refine`)
// sends a real `If-Match` header on every update/delete here, built from
// whichever version it last saw for this record (README's "Optimistic
// locking" section) — verified against the live WireMock container (see
// this example's README, "Verification"). Since #605 the mock ENFORCES it,
// mirroring the real server: a stale or absent `If-Match` gets a 412, and
// a successful update bumps `version` and returns the new `ETag`. Guarded
// by `../../tests/smoke.rs`'s
// `wiremock_stubs_enforce_if_match_on_the_versioned_model` and asserted
// live in `web/scripts/verify.ts`, so a regression fails CI rather than
// silently turning this page's conflict handling into decoration.
import { useCreate, useDelete, useList, useUpdate } from "@refinedev/core";
import { type FormEvent, useState } from "react";
import type { Post } from "../../generated/src/models.js";
import { QueryState } from "./QueryState.js";

type Edit = { title: string; published: boolean };

export function PostsPage() {
  const { result, query } = useList<Post>({ resource: "posts", pagination: { mode: "off" } });
  // See CategoriesPage.tsx's comment: refine v5's mutation hooks nest
  // `isPending`/etc. under `.mutation`, not at the top level.
  const { mutation: createMutation, mutate: create } = useCreate<Post>();
  const { mutate: update } = useUpdate<Post>();
  const { mutate: remove } = useDelete();

  const [newId, setNewId] = useState("");
  const [newTitle, setNewTitle] = useState("");
  const [newPublished, setNewPublished] = useState(false);
  const [edits, setEdits] = useState<Record<number, Edit>>({});

  function onCreate(event: FormEvent) {
    event.preventDefault();
    const id = Number(newId);
    if (!id || !newTitle.trim()) return;
    create(
      { resource: "posts", values: { id, title: newTitle.trim(), published: newPublished } },
      {
        onSuccess: () => {
          setNewId("");
          setNewTitle("");
          setNewPublished(false);
        },
      },
    );
  }

  return (
    <section>
      <h2>
        Posts <span className="hint">`@@paged` + `@version` — optimistic-locking wiring</span>
      </h2>
      <p className="callout">
        Optimistic locking is live here: this backend enforces <code>If-Match</code>, so saving a
        row that someone else already changed conflicts with a <code>412</code> instead of
        silently overwriting. List filtering, sorting and pagination are <em>not</em> implemented
        by the mock, which is why this table offers none.
      </p>
      <QueryState isLoading={query.isLoading} isError={query.isError} error={query.error} />
      <table>
        <thead>
          <tr>
            <th>id</th>
            <th>title</th>
            <th>published</th>
            <th>version</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {result.data.map((post) => {
            const id = post.id!;
            const edit = edits[id] ?? {
              title: post.title ?? "",
              published: post.published ?? false,
            };
            const setEdit = (patch: Partial<Edit>) =>
              setEdits((prev) => ({ ...prev, [id]: { ...edit, ...patch } }));
            return (
              <tr key={id}>
                <td>{id}</td>
                <td>
                  <input value={edit.title} onChange={(e) => setEdit({ title: e.target.value })} />
                </td>
                <td>
                  <input
                    type="checkbox"
                    checked={edit.published}
                    onChange={(e) => setEdit({ published: e.target.checked })}
                  />
                </td>
                <td>{post.version}</td>
                <td>
                  <button
                    type="button"
                    onClick={() => update({ resource: "posts", id, values: edit })}
                  >
                    Save
                  </button>
                  <button
                    type="button"
                    className="danger"
                    onClick={() => remove({ resource: "posts", id })}
                  >
                    Delete
                  </button>
                </td>
              </tr>
            );
          })}
          {result.data.length === 0 && !query.isLoading && (
            <tr>
              <td colSpan={5} className="muted">
                No posts yet — add one below.
              </td>
            </tr>
          )}
        </tbody>
      </table>
      <form onSubmit={onCreate} className="create-form">
        <input placeholder="id" value={newId} onChange={(e) => setNewId(e.target.value)} />
        <input placeholder="title" value={newTitle} onChange={(e) => setNewTitle(e.target.value)} />
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={newPublished}
            onChange={(e) => setNewPublished(e.target.checked)}
          />
          published
        </label>
        <button type="submit" disabled={createMutation.isPending}>
          Add post
        </button>
      </form>
    </section>
  );
}
