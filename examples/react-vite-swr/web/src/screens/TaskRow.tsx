import type { Task } from "react-vite-swr-client";
import { useDeleteTask, useUpdateTask } from "react-vite-swr-client/swr/models/task.hooks";
import { runtime } from "../runtime.ts";

// Split out of `BoardDetailScreen` because `useUpdateTask`/`useDeleteTask`
// bind their `id` argument at hook-call time (`useUpdateTask(runtime, id)`,
// not `trigger({ id, ...patch })`) — one row's mutation hooks would
// otherwise mean calling a hook inside `.map()` with a changing id,
// which breaks React's rules of hooks. One `TaskRow` component per task
// keeps each hook call's `id` stable for that component's lifetime,
// which is the correct — if slightly non-obvious the first time you hit
// it — way to use these hooks in a list. Worth knowing before you reach
// for this pattern; see the PR description for more on this ergonomic
// wrinkle.
export function TaskRow({ task }: { task: Task }) {
  const { trigger: updateTask, isMutating: isToggling } = useUpdateTask(runtime, task.id as number);
  const { trigger: deleteTask, isMutating: isDeleting } = useDeleteTask(runtime, task.id as number);

  return (
    <li className={`task-row${task.done ? " done" : ""}`}>
      <label className="row">
        <input
          type="checkbox"
          checked={task.done ?? false}
          disabled={isToggling}
          onChange={(event) => updateTask({ done: event.target.checked })}
        />
        <span>{task.title}</span>
      </label>
      <button
        type="button"
        className="link-button"
        disabled={isDeleting}
        onClick={() => deleteTask()}
      >
        {isDeleting ? "Deleting…" : "Delete"}
      </button>
    </li>
  );
}
