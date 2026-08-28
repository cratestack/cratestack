// Generated per-model module for the `swr` preset (issue #304): this
// model's types plus a plain, framework-free `async` function per CRUD
// operation. No client class, no React import — every function takes a
// `CratestackRpcRuntime` as its first argument, so it's callable from a
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

import type { CratestackRpcRuntime, CratestackRpcCallOptions } from "../runtime.js";
import { toRpcListInput, type CratestackRpcListQuery } from "../queries.js";
// cratestack#498: see `models-rest.ts.j2`'s identical import for why
// `Decimal` is unconditional and `reviveWireFields` is a real (not
// type-only) import.
import { reviveWireFields, revivePagedWireFields, type Decimal } from "./shared.js";
import type { BooleanFilter, ComparableFilter, DateTimeFilter, DecimalFilter, EqualityFilter, NumberFilter, SortDirection, StringFilter, UuidFilter } from "./shared.js";

export type WidgetSortField = 'id' | 'name' | 'weight';
export const WidgetSortFieldValues = [
  "id",
  "name",
  "weight",
] as const satisfies readonly WidgetSortField[];

export interface WidgetWhere {
  id?: NumberFilter;
  name?: StringFilter;
  weight?: NumberFilter;
}

export interface WidgetOrderByClause {
  field: WidgetSortField;
  direction: SortDirection;
}

export interface WidgetFindMany {
  where?: WidgetWhere;
  orderBy?: WidgetOrderByClause[];
}

export interface Widget {
  id?: number;
  name?: string;
  weight?: number | null;
}

export interface CreateWidgetInput {
  id: number;
  name: string;
  weight?: number | null;
}

export interface UpdateWidgetInput {
  name?: string;
  weight?: number | null;
}

export async function listWidgets(
  runtime: CratestackRpcRuntime,
  query: CratestackRpcListQuery = {},
  options: CratestackRpcCallOptions = {},
): Promise<Widget[]> {
  return runtime.call<Record<string, unknown>, unknown>(
    "model.Widget.list",
    toRpcListInput(query),
    options,
  ).then((value) => reviveWireFields(value, 'Widget') as Widget[]);
}

export async function getWidget(
  runtime: CratestackRpcRuntime,
  id: number,
  options: CratestackRpcCallOptions = {},
): Promise<Widget> {
  return runtime.call<{ id: number }, unknown>(
    "model.Widget.get",
    { id },
    options,
  ).then((value) => reviveWireFields(value, 'Widget') as Widget);
}

export async function createWidget(
  runtime: CratestackRpcRuntime,
  input: CreateWidgetInput,
  options: CratestackRpcCallOptions = {},
): Promise<Widget> {
  return runtime.call<CreateWidgetInput, unknown>(
    "model.Widget.create",
    input,
    options,
  ).then((value) => reviveWireFields(value, 'Widget') as Widget);
}

export async function updateWidget(
  runtime: CratestackRpcRuntime,
  id: number,
  patch: UpdateWidgetInput,
  options: CratestackRpcCallOptions = {},
): Promise<Widget> {
  return runtime.call<{ id: number; patch: UpdateWidgetInput }, unknown>(
    "model.Widget.update",
    { id, patch },
    options,
  ).then((value) => reviveWireFields(value, 'Widget') as Widget);
}

export async function deleteWidget(
  runtime: CratestackRpcRuntime,
  id: number,
  options: CratestackRpcCallOptions = {},
): Promise<void> {
  return runtime.call<{ id: number }, void>(
    "model.Widget.delete",
    { id },
    options,
  );
}