// The plain case: `@id` named `id`, no `@@paged`, no `@version`. Contrast
// this file against PostsPage.tsx (paged + versioned) and TagsPage.tsx
// (`@id` not named `id`) to see what actually differs.
//
// `pagination: { mode: "off" }` on every list in this app is deliberate,
// not an oversight — see README.md's "Sorting, filtering, pagination"
// section: the generated WireMock stubs ignore `limit`/`offset`/`sort`/
// `field__operator` entirely, so building working-looking controls for
// them would silently lie about what the mock does.
import { useCreate, useDelete, useList, useUpdate } from "@refinedev/core";
import { type FormEvent, useState } from "react";
import type { Category } from "../../generated/src/models.js";
import { QueryState } from "./QueryState.js";

export function CategoriesPage() {
  const { result, query } = useList<Category>({
    resource: "categories",
    pagination: { mode: "off" },
  });
  // refine v5's mutation hooks return `{ mutation, mutate, mutateAsync }`
  // — `mutate`/`mutateAsync` are lifted to the top level for convenience,
  // but `isPending`/`isError`/etc. stay nested under `mutation` (refine's
  // own `UseMutationResult` wrapper type, not react-query's raw one).
  const { mutation: createMutation, mutate: create } = useCreate<Category>();
  const { mutate: update } = useUpdate<Category>();
  const { mutate: remove } = useDelete();

  const [newId, setNewId] = useState("");
  const [newName, setNewName] = useState("");
  const [edits, setEdits] = useState<Record<number, string>>({});

  function onCreate(event: FormEvent) {
    event.preventDefault();
    const id = Number(newId);
    if (!id || !newName.trim()) return;
    create(
      { resource: "categories", values: { id, name: newName.trim() } },
      {
        onSuccess: () => {
          setNewId("");
          setNewName("");
        },
      },
    );
  }

  return (
    <section>
      <h2>
        Categories <span className="hint">plain CRUD — primary key `id`</span>
      </h2>
      <QueryState isLoading={query.isLoading} isError={query.isError} error={query.error} />
      <table>
        <thead>
          <tr>
            <th>id</th>
            <th>name</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {result.data.map((category) => {
            const id = category.id!;
            const value = edits[id] ?? category.name ?? "";
            return (
              <tr key={id}>
                <td>{id}</td>
                <td>
                  <input
                    value={value}
                    onChange={(e) => setEdits((prev) => ({ ...prev, [id]: e.target.value }))}
                  />
                </td>
                <td>
                  <button
                    type="button"
                    onClick={() => update({ resource: "categories", id, values: { name: value } })}
                  >
                    Save
                  </button>
                  <button
                    type="button"
                    className="danger"
                    onClick={() => remove({ resource: "categories", id })}
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
                No categories yet — add one below.
              </td>
            </tr>
          )}
        </tbody>
      </table>
      <form onSubmit={onCreate} className="create-form">
        <input placeholder="id" value={newId} onChange={(e) => setNewId(e.target.value)} />
        <input placeholder="name" value={newName} onChange={(e) => setNewName(e.target.value)} />
        <button type="submit" disabled={createMutation.isPending}>
          Add category
        </button>
      </form>
    </section>
  );
}
