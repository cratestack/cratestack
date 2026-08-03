// Generated per-model module for the `swr` preset (issue #304): this
// model's types plus a plain, framework-free `async` function per CRUD
// operation. No client class, no React import — every function takes a
// `CratestackRuntime` as its first argument, so it's callable from a
// React component, a server action, a plain Node script, or a test.
//
// Ownership rule: a type used by exactly one model is defined here,
// inline; a type used by two or more models (or referenced only by a
// procedure, or declared but unused) lives in `./shared` and is imported
// instead. See `cratestack-client-typescript`'s
// `src/swr/ownership.rs::compute_type_ownership` for the computation
// that decided this file's inline types vs. its imports. Imports of
// another model's own type (a relation field, e.g. `author: User`) are
// always `import type` — never a value import — so two models that
// reference each other (a relation cycle) can never become a runtime
// import cycle, only a type-only one, which TypeScript tolerates fine.

import type { CratestackRuntime } from "../runtime.js";
import { toSearchQuery, type CratestackQueryRequestConfig, type CratestackRequestConfig } from "../queries.js";
import type { BooleanFilter, ComparableFilter, DateTimeFilter, DecimalFilter, EqualityFilter, NumberFilter, SortDirection, StringFilter, UuidFilter } from "./shared.js";
import type { Board } from "./board.js";

export type TaskSortField = 'id' | 'title' | 'done' | 'boardId';
export const TaskSortFieldValues = [
  "id",
  "title",
  "done",
  "boardId",
] as const satisfies readonly TaskSortField[];

export interface TaskWhere {
  id?: NumberFilter;
  title?: StringFilter;
  done?: BooleanFilter;
  boardId?: NumberFilter;
}

export interface TaskOrderByClause {
  field: TaskSortField;
  direction: SortDirection;
}

export interface TaskFindMany {
  where?: TaskWhere;
  orderBy?: TaskOrderByClause[];
}

export interface Task {
  id?: number;
  title?: string;
  done?: boolean;
  boardId?: number;
  board?: Board;
}

export interface CreateTaskInput {
  id: number;
  title: string;
  done: boolean;
  boardId: number;
}

export interface UpdateTaskInput {
  title?: string;
  done?: boolean;
  boardId?: number;
}

export async function listTasks(
  runtime: CratestackRuntime,
  options: CratestackQueryRequestConfig = {},
): Promise<Task[]> {
  return runtime.get<Task[]>("/tasks", {
    headers: options.headers,
    query: toSearchQuery(options.query),
    signal: options.signal,
  });
}

export async function getTask(
  runtime: CratestackRuntime,
  id: number,
  options: CratestackQueryRequestConfig = {},
): Promise<Task> {
  return runtime.get<Task>(`/tasks/${encodeURIComponent(String(id))}`, {
    headers: options.headers,
    query: toSearchQuery(options.query),
    signal: options.signal,
  });
}

export async function createTask(
  runtime: CratestackRuntime,
  input: CreateTaskInput,
  options: CratestackRequestConfig = {},
): Promise<Task> {
  return runtime.post<Task>("/tasks", input, options);
}

export async function updateTask(
  runtime: CratestackRuntime,
  id: number,
  input: UpdateTaskInput,
  options: CratestackRequestConfig = {},
): Promise<Task> {
  return runtime.patch<Task>(`/tasks/${encodeURIComponent(String(id))}`, input, options);
}

export async function deleteTask(
  runtime: CratestackRuntime,
  id: number,
  options: CratestackRequestConfig = {},
): Promise<void> {
  return runtime.delete<void>(`/tasks/${encodeURIComponent(String(id))}`, options);
}