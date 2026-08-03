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

export type BoardSortField = 'id' | 'name';
export const BoardSortFieldValues = [
  "id",
  "name",
] as const satisfies readonly BoardSortField[];

export interface BoardWhere {
  id?: NumberFilter;
  name?: StringFilter;
}

export interface BoardOrderByClause {
  field: BoardSortField;
  direction: SortDirection;
}

export interface BoardFindMany {
  where?: BoardWhere;
  orderBy?: BoardOrderByClause[];
}

export interface Board {
  id?: number;
  name?: string;
}

export interface CreateBoardInput {
  id: number;
  name: string;
}

export interface UpdateBoardInput {
  name?: string;
}

export async function listBoards(
  runtime: CratestackRuntime,
  options: CratestackQueryRequestConfig = {},
): Promise<Board[]> {
  return runtime.get<Board[]>("/boards", {
    headers: options.headers,
    query: toSearchQuery(options.query),
    signal: options.signal,
  });
}

export async function getBoard(
  runtime: CratestackRuntime,
  id: number,
  options: CratestackQueryRequestConfig = {},
): Promise<Board> {
  return runtime.get<Board>(`/boards/${encodeURIComponent(String(id))}`, {
    headers: options.headers,
    query: toSearchQuery(options.query),
    signal: options.signal,
  });
}

export async function createBoard(
  runtime: CratestackRuntime,
  input: CreateBoardInput,
  options: CratestackRequestConfig = {},
): Promise<Board> {
  return runtime.post<Board>("/boards", input, options);
}

export async function updateBoard(
  runtime: CratestackRuntime,
  id: number,
  input: UpdateBoardInput,
  options: CratestackRequestConfig = {},
): Promise<Board> {
  return runtime.patch<Board>(`/boards/${encodeURIComponent(String(id))}`, input, options);
}

export async function deleteBoard(
  runtime: CratestackRuntime,
  id: number,
  options: CratestackRequestConfig = {},
): Promise<void> {
  return runtime.delete<void>(`/boards/${encodeURIComponent(String(id))}`, options);
}