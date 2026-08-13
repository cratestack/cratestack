// `@@paged` + `@version`. The refine dataProvider (`@cratestack/refine`)
// sends a real `If-Match` header on every update/delete here, built from
// whichever version it last saw for this record (README's "Optimistic
// locking" section) — verified against the live WireMock container (see
// this example's README, "Verification"). What that verification also
// shows: THIS MOCK DOES NOT ENFORCE IT. `cratestack-mock-wiremock`'s
// generated stubs match on method + path only (no request-header
// matching anywhere in that generator — confirmed in
// `../../tests/smoke.rs`'s `wiremock_stubs_do_not_validate_if_match_or_any_request_header`
// test), and never increments `version` server-side either. A stale
// update here succeeds instead of conflicting with a 412. The callout
// below says so; don't remove it without re-verifying against a real
// `cratestack-pg` server or an updated `cratestack-mock-wiremock`.
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
        This mock backend does not check <code>If-Match</code> — a stale save below will succeed,
        not conflict. See README.md's "What this demo can't prove".
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
