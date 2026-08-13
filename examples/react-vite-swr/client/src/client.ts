import { CratestackRuntime, type CratestackClientOptions } from "./runtime.js";
import { reviveDecimalFields, revivePagedDecimalFields, reviveDecimalScalar } from "./models.js";
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
import { toSearchQuery, type CratestackQueryRequestConfig, type CratestackRequestConfig } from "./queries.js";

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
    }).then((value) => reviveDecimalFields(value, 'Board') as Board[]);
  }

  get(id: number, options: CratestackQueryRequestConfig = {}): Promise<Board> {
    return this.runtime.get<unknown>(`/boards/${encodeURIComponent(String(id))}`, {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((value) => reviveDecimalFields(value, 'Board') as Board);
  }

  create(input: CreateBoardInput, options: CratestackRequestConfig = {}): Promise<Board> {
    return this.runtime.post<unknown>("/boards", input, options)
      .then((value) => reviveDecimalFields(value, 'Board') as Board);
  }

  update(
    id: number,
    input: UpdateBoardInput,
    options: CratestackRequestConfig = {},
  ): Promise<Board> {
    return this.runtime.patch<unknown>(`/boards/${encodeURIComponent(String(id))}`, input, options)
      .then((value) => reviveDecimalFields(value, 'Board') as Board);
  }

  delete(id: number, options: CratestackRequestConfig = {}): Promise<void> {
    return this.runtime.delete<void>(`/boards/${encodeURIComponent(String(id))}`, options);
  }
}

export class TaskApi {
  constructor(private readonly runtime: CratestackRuntime) {}

  list(options: CratestackQueryRequestConfig = {}): Promise<Task[]> {
    return this.runtime.get<unknown>("/tasks", {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((value) => reviveDecimalFields(value, 'Task') as Task[]);
  }

  get(id: number, options: CratestackQueryRequestConfig = {}): Promise<Task> {
    return this.runtime.get<unknown>(`/tasks/${encodeURIComponent(String(id))}`, {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((value) => reviveDecimalFields(value, 'Task') as Task);
  }

  create(input: CreateTaskInput, options: CratestackRequestConfig = {}): Promise<Task> {
    return this.runtime.post<unknown>("/tasks", input, options)
      .then((value) => reviveDecimalFields(value, 'Task') as Task);
  }

  update(
    id: number,
    input: UpdateTaskInput,
    options: CratestackRequestConfig = {},
  ): Promise<Task> {
    return this.runtime.patch<unknown>(`/tasks/${encodeURIComponent(String(id))}`, input, options)
      .then((value) => reviveDecimalFields(value, 'Task') as Task);
  }

  delete(id: number, options: CratestackRequestConfig = {}): Promise<void> {
    return this.runtime.delete<void>(`/tasks/${encodeURIComponent(String(id))}`, options);
  }
}

export class ProceduresApi {
  constructor(private readonly runtime: CratestackRuntime) {}

  estimateFocusMinutes(args: EstimateFocusMinutesArgs, options: CratestackRequestConfig = {}): Promise<FocusEstimateResult> {
    return this.runtime.post<unknown>("/$procs/estimateFocusMinutes", args, options)
      .then((value) => reviveDecimalFields(value, 'FocusEstimateResult') as FocusEstimateResult);
  }

}