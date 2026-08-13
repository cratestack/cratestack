import { type FormEvent, useMemo, useState } from "react";
import { useBoard } from "react-vite-swr-client/swr/models/board.hooks";
import { useCreateTask, useTasks } from "react-vite-swr-client/swr/models/task.hooks";
import { useEstimateFocusMinutesQuery } from "react-vite-swr-client/swr/procedures.hooks";
import { runtime } from "../runtime.ts";
import { TaskRow } from "./TaskRow.tsx";

const MINUTES_PER_TASK = 25; // one pomodoro — the estimate procedure's own unit.

export function BoardDetailScreen({ boardId, onBack }: { boardId: number; onBack: () => void }) {
  const { data: board } = useBoard(runtime, boardId);
  // The generated model functions have no server-side filter for "tasks
  // on this board" wired up yet (see the README's scope note on
  // `where`/structured filters), so this reads the full task list — the
  // hook itself is still 100% generated — and filters client-side.
  const { data: allTasks, error, isLoading } = useTasks(runtime);
  const { trigger: createTask, isMutating } = useCreateTask(runtime);
  const [title, setTitle] = useState("");

  const tasks = useMemo(
    () => allTasks?.filter((task) => task.boardId === boardId) ?? [],
    [allTasks, boardId],
  );
  const openCount = tasks.filter((task) => !task.done).length;

  // A query-kind procedure hook: conditional on `openCount` being known
  // (`null` before the task list has loaded skips the request, same
  // idiom as a model detail hook with a not-yet-known id).
  const { data: estimate } = useEstimateFocusMinutesQuery(
    runtime,
    allTasks ? { args: { taskCount: openCount, minutesPerTask: MINUTES_PER_TASK } } : null,
  );

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = title.trim();
    if (!trimmed) return;
    await createTask({ id: Date.now(), title: trimmed, done: false, boardId });
    setTitle("");
  }

  return (
    <section>
      <button type="button" className="link-button" onClick={onBack}>
        ← Boards
      </button>
      <h1>{board?.name ?? "…"}</h1>

      {estimate && (
        <p className="muted">
          {openCount} open task{openCount === 1 ? "" : "s"} ≈ {estimate.totalMinutes} focus minutes
          ({MINUTES_PER_TASK} min/task, via the <code>estimateFocusMinutes</code> procedure hook)
        </p>
      )}

      <form className="row" onSubmit={onSubmit}>
        <input
          type="text"
          placeholder="New task title"
          value={title}
          onChange={(event) => setTitle(event.target.value)}
          maxLength={200}
        />
        <button type="submit" disabled={isMutating || title.trim().length === 0}>
          {isMutating ? "Adding…" : "Add task"}
        </button>
      </form>

      {isLoading && <p className="muted">Loading tasks…</p>}
      {error && <p className="error">Failed to load tasks: {error.message}</p>}
      {!isLoading && tasks.length === 0 && <p className="muted">No tasks on this board yet.</p>}

      <ul className="list">
        {tasks.map((task) => (
          <TaskRow key={task.id} task={task} />
        ))}
      </ul>
    </section>
  );
}
