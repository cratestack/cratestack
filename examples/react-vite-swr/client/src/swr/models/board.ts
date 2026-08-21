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

import type { CratestackRuntime, CratestackResponseEnvelope } from "../runtime.js";
import {
  toSearchQuery,
  withIfMatchHeader,
  type CratestackQueryRequestConfig,
  type CratestackRequestConfig,
  type CratestackWriteRequestConfig,
} from "../queries.js";
// cratestack#498: every generated model file gets this import
// unconditionally (like `../runtime.js`/`../queries.js` above), whether
// or not this particular model has a `Decimal` field — the alternative
// is threading a per-model "does it need Decimal" flag through the
// ownership computation for a type-only import that costs nothing when
// unused (this package's `tsconfig.json.j2` doesn't set
// `noUnusedLocals`). `reviveDecimalFields` is a real (non-type) import:
// every function below that decodes a server response calls it, same as
// the `default` preset's `rest-client.ts.j2`.
import { reviveDecimalFields, revivePagedDecimalFields, type Decimal } from "./shared.js";
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
  return runtime.get<unknown>("/boards", {
    headers: options.headers,
    query: toSearchQuery(options.query),
    signal: options.signal,
  }).then((value) => reviveDecimalFields(value, 'Board') as Board[]);
}

export async function getBoard(
  runtime: CratestackRuntime,
  id: number,
  options: CratestackQueryRequestConfig = {},
): Promise<Board> {
  return runtime.get<unknown>(`/boards/${encodeURIComponent(String(id))}`, {
    headers: options.headers,
    query: toSearchQuery(options.query),
    signal: options.signal,
  }).then((value) => reviveDecimalFields(value, 'Board') as Board);
}

// Same call as `getBoard`, but returns the response alongside the
// record (issue #610) — read `.response.headers.get("etag")` off the
// result to get the value `updateBoard`/`deleteBoard`'s `ifMatch`
// option needs. Applies the same decimal revival `getBoard` does —
// reaching for the raw `runtime.getWithResponse()` instead would skip
// it and hand back an unrevived (string, not `Decimal`) value.
export async function getBoardWithResponse(
  runtime: CratestackRuntime,
  id: number,
  options: CratestackQueryRequestConfig = {},
): Promise<CratestackResponseEnvelope<Board>> {
  return runtime.getWithResponse<unknown>(`/boards/${encodeURIComponent(String(id))}`, {
    headers: options.headers,
    query: toSearchQuery(options.query),
    signal: options.signal,
  }).then((result) => ({
    value: reviveDecimalFields(result.value, 'Board') as Board,
    response: result.response,
  }));
}

export async function createBoard(
  runtime: CratestackRuntime,
  input: CreateBoardInput,
  options: CratestackRequestConfig = {},
): Promise<Board> {
  return runtime.post<unknown>("/boards", input, options)
    .then((value) => reviveDecimalFields(value, 'Board') as Board);
}

export async function updateBoard(
  runtime: CratestackRuntime,
  id: number,
  input: UpdateBoardInput,
  options: CratestackWriteRequestConfig = {},
): Promise<Board> {
  return runtime.patch<unknown>(`/boards/${encodeURIComponent(String(id))}`, input, {
    headers: withIfMatchHeader(options.headers, options.ifMatch),
    signal: options.signal,
  })
    .then((value) => reviveDecimalFields(value, 'Board') as Board);
}

export async function deleteBoard(
  runtime: CratestackRuntime,
  id: number,
  options: CratestackWriteRequestConfig = {},
): Promise<void> {
  return runtime.delete<void>(`/boards/${encodeURIComponent(String(id))}`, {
    headers: withIfMatchHeader(options.headers, options.ifMatch),
    signal: options.signal,
  });
}