import { CratestackRuntime, type CratestackClientOptions, type CratestackResponseEnvelope } from "./runtime.js";
import { reviveWireFields, revivePagedWireFields, reviveWireScalar } from "./models.js";
import type {
  FocusEstimateArgs,
  FocusEstimateResult,
  Board,
  CreateBoardInput,
  UpdateBoardInput,
  BoardWhere,
  BoardOrderByClause,
  BoardFindMany,
  Task,
  CreateTaskInput,
  UpdateTaskInput,
  TaskWhere,
  TaskOrderByClause,
  TaskFindMany,
  EstimateFocusMinutesArgs,
  BoardSortField,
  TaskSortField,
  Page,
} from "./models.js";
import {
  toSearchQuery,
  withIfMatchHeader,
  type CratestackQueryRequestConfig,
  type CratestackRequestConfig,
  type CratestackWriteRequestConfig,
} from "./queries.js";

export class ReactViteSwrClientClient {
  readonly runtime: CratestackRuntime;
  readonly procedures: ProceduresApi;
  readonly boards: BoardApi;
  readonly tasks: TaskApi;

  constructor(originOrRuntime: string | CratestackRuntime, options: CratestackClientOptions = {}) {
    this.runtime = typeof originOrRuntime === "string"
      ? new CratestackRuntime(originOrRuntime, options)
      : originOrRuntime;
    this.procedures = new ProceduresApi(this.runtime);
    this.boards = new BoardApi(this.runtime);
    this.tasks = new TaskApi(this.runtime);
  }
}

export class BoardApi {
  constructor(private readonly runtime: CratestackRuntime) {}

  list(options: CratestackQueryRequestConfig = {}): Promise<Board[]> {
    return this.runtime.get<unknown>("/boards", {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((value) => reviveWireFields(value, 'Board') as Board[]);
  }

  get(id: number, options: CratestackQueryRequestConfig = {}): Promise<Board> {
    return this.runtime.get<unknown>(`/boards/${encodeURIComponent(String(id))}`, {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((value) => reviveWireFields(value, 'Board') as Board);
  }

  // Same call as `get`, but returns the response alongside the record
  // (issue #610) — read `.response.headers.get("etag")` off the result
  // to get the value `update`/`delete`'s `ifMatch` option needs.
  getWithResponse(
    id: number,
    options: CratestackQueryRequestConfig = {},
  ): Promise<CratestackResponseEnvelope<Board>> {
    return this.runtime.getWithResponse<unknown>(`/boards/${encodeURIComponent(String(id))}`, {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((result) => ({
      value: reviveWireFields(result.value, 'Board') as Board,
      response: result.response,
    }));
  }

  create(input: CreateBoardInput, options: CratestackRequestConfig = {}): Promise<Board> {
    return this.runtime.post<unknown>("/boards", input, options)
      .then((value) => reviveWireFields(value, 'Board') as Board);
  }

  update(
    id: number,
    input: UpdateBoardInput,
    options: CratestackWriteRequestConfig = {},
  ): Promise<Board> {
    return this.runtime.patch<unknown>(`/boards/${encodeURIComponent(String(id))}`, input, {
      headers: withIfMatchHeader(options.headers, options.ifMatch),
      signal: options.signal,
    })
      .then((value) => reviveWireFields(value, 'Board') as Board);
  }

  delete(id: number, options: CratestackWriteRequestConfig = {}): Promise<void> {
    return this.runtime.delete<void>(`/boards/${encodeURIComponent(String(id))}`, {
      headers: withIfMatchHeader(options.headers, options.ifMatch),
      signal: options.signal,
    });
  }
}

export class TaskApi {
  constructor(private readonly runtime: CratestackRuntime) {}

  list(options: CratestackQueryRequestConfig = {}): Promise<Task[]> {
    return this.runtime.get<unknown>("/tasks", {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((value) => reviveWireFields(value, 'Task') as Task[]);
  }

  get(id: number, options: CratestackQueryRequestConfig = {}): Promise<Task> {
    return this.runtime.get<unknown>(`/tasks/${encodeURIComponent(String(id))}`, {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((value) => reviveWireFields(value, 'Task') as Task);
  }

  // Same call as `get`, but returns the response alongside the record
  // (issue #610) — read `.response.headers.get("etag")` off the result
  // to get the value `update`/`delete`'s `ifMatch` option needs.
  getWithResponse(
    id: number,
    options: CratestackQueryRequestConfig = {},
  ): Promise<CratestackResponseEnvelope<Task>> {
    return this.runtime.getWithResponse<unknown>(`/tasks/${encodeURIComponent(String(id))}`, {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((result) => ({
      value: reviveWireFields(result.value, 'Task') as Task,
      response: result.response,
    }));
  }

  create(input: CreateTaskInput, options: CratestackRequestConfig = {}): Promise<Task> {
    return this.runtime.post<unknown>("/tasks", input, options)
      .then((value) => reviveWireFields(value, 'Task') as Task);
  }

  update(
    id: number,
    input: UpdateTaskInput,
    options: CratestackWriteRequestConfig = {},
  ): Promise<Task> {
    return this.runtime.patch<unknown>(`/tasks/${encodeURIComponent(String(id))}`, input, {
      headers: withIfMatchHeader(options.headers, options.ifMatch),
      signal: options.signal,
    })
      .then((value) => reviveWireFields(value, 'Task') as Task);
  }

  delete(id: number, options: CratestackWriteRequestConfig = {}): Promise<void> {
    return this.runtime.delete<void>(`/tasks/${encodeURIComponent(String(id))}`, {
      headers: withIfMatchHeader(options.headers, options.ifMatch),
      signal: options.signal,
    });
  }
}

export class ProceduresApi {
  constructor(private readonly runtime: CratestackRuntime) {}

  estimateFocusMinutes(args: EstimateFocusMinutesArgs, options: CratestackRequestConfig = {}): Promise<FocusEstimateResult> {
    return this.runtime.post<unknown>("/$procs/estimateFocusMinutes", args, options)
      .then((value) => reviveWireFields(value, 'FocusEstimateResult') as FocusEstimateResult);
  }

}