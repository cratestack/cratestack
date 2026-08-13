// `@id` is `slug`, not `id`. `@cratestack/refine` still synthesizes a
// client-side `id` field on every returned record (README's "Primary
// keys" section) for refine's own row-keying machinery, but a *create*
// payload has no record yet to synthesize one from — the form below
// fields the real primary-key name, `slug`, same as the README warns.
import { useCreate, useDelete, useList, useUpdate } from "@refinedev/core";
import { type FormEvent, useState } from "react";
import type { Tag } from "../../generated/src/models.js";
import { QueryState } from "./QueryState.js";

export function TagsPage() {
  const { result, query } = useList<Tag>({ resource: "tags", pagination: { mode: "off" } });
  // See CategoriesPage.tsx's comment: refine v5's mutation hooks nest
  // `isPending`/etc. under `.mutation`, not at the top level.
  const { mutation: createMutation, mutate: create } = useCreate<Tag>();
  const { mutate: update } = useUpdate<Tag>();
  const { mutate: remove } = useDelete();

  const [newSlug, setNewSlug] = useState("");
  const [newLabel, setNewLabel] = useState("");
  const [edits, setEdits] = useState<Record<string, string>>({});

  function onCreate(event: FormEvent) {
    event.preventDefault();
    if (!newSlug.trim() || !newLabel.trim()) return;
    create(
      { resource: "tags", values: { slug: newSlug.trim(), label: newLabel.trim() } },
      {
        onSuccess: () => {
          setNewSlug("");
          setNewLabel("");
        },
      },
    );
  }

  return (
    <section>
      <h2>
        Tags <span className="hint">`@id` is `slug`, not `id`</span>
      </h2>
      <QueryState isLoading={query.isLoading} isError={query.isError} error={query.error} />
      <table>
        <thead>
          <tr>
            <th>slug</th>
            <th>label</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {result.data.map((tag) => {
            const slug = tag.slug!;
            const value = edits[slug] ?? tag.label ?? "";
            return (
              <tr key={slug}>
                <td>{slug}</td>
                <td>
                  <input
                    value={value}
                    onChange={(e) => setEdits((prev) => ({ ...prev, [slug]: e.target.value }))}
                  />
                </td>
                <td>
                  <button
                    type="button"
                    onClick={() => update({ resource: "tags", id: slug, values: { label: value } })}
                  >
                    Save
                  </button>
                  <button
                    type="button"
                    className="danger"
                    onClick={() => remove({ resource: "tags", id: slug })}
                  >
                    Delete
                  </button>
                </td>
              </tr>
            );
          })}
          {result.data.length === 0 && !query.isLoading && (
            <tr>
              <td colSpan={3} className="muted">
                No tags yet — add one below.
              </td>
            </tr>
          )}
        </tbody>
      </table>
      <form onSubmit={onCreate} className="create-form">
        <input placeholder="slug" value={newSlug} onChange={(e) => setNewSlug(e.target.value)} />
        <input placeholder="label" value={newLabel} onChange={(e) => setNewLabel(e.target.value)} />
        <button type="submit" disabled={createMutation.isPending}>
          Add tag
        </button>
      </form>
    </section>
  );
}
