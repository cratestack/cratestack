import { type FormEvent, useState } from "react";
// Every hook here is generated — nothing in this file hand-writes a
// `useSWR` call, a cache key, or a fetcher. `useBoards` reads the list;
// `useCreateBoard` is a mutation whose success invalidates that same
// list (see `client/src/swr/models/board.hooks.ts`'s own header comment
// for the fixed invalidation rule) — that invalidation is what makes the
// list below refresh the instant a board is created, with no manual
// `mutate()`/refetch call anywhere in this component.
import { useBoards, useCreateBoard } from "react-vite-swr-client/swr/models/board.hooks";
import { runtime } from "../runtime.ts";

export function BoardsScreen({ onSelectBoard }: { onSelectBoard: (id: number) => void }) {
  const { data: boards, error, isLoading } = useBoards(runtime);
  const { trigger: createBoard, isMutating } = useCreateBoard(runtime);
  const [name, setName] = useState("");

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) return;
    // The id is client-supplied (the schema declares `id Int @id` with
    // no default) — `Date.now()` is the same trick every other example
    // in this repo uses for a client-generated numeric id.
    await createBoard({ id: Date.now(), name: trimmed });
    setName("");
  }

  return (
    <section>
      <h1>Boards</h1>

      <form className="row" onSubmit={onSubmit}>
        <input
          type="text"
          placeholder="New board name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          maxLength={200}
        />
        <button type="submit" disabled={isMutating || name.trim().length === 0}>
          {isMutating ? "Adding…" : "Add board"}
        </button>
      </form>

      {isLoading && <p className="muted">Loading boards…</p>}
      {error && <p className="error">Failed to load boards: {error.message}</p>}

      {boards && boards.length === 0 && <p className="muted">No boards yet — add one above.</p>}

      <ul className="list">
        {boards?.map((board) => (
          <li key={board.id}>
            <button
              type="button"
              className="list-row"
              onClick={() => board.id != null && onSelectBoard(board.id)}
            >
              {board.name}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
