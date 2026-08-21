import { CratestackRuntime, type CratestackClientOptions, type CratestackResponseEnvelope } from "./runtime.js";
import { reviveDecimalFields, revivePagedDecimalFields, reviveDecimalScalar } from "./models.js";
import type {
  Widget,
  CreateWidgetInput,
  UpdateWidgetInput,
  WidgetWhere,
  WidgetOrderByClause,
  WidgetFindMany,
  EchoNameArgs,
  WidgetSortField,
  Page,
} from "./models.js";
import {
  toSearchQuery,
  withIfMatchHeader,
  type CratestackQueryRequestConfig,
  type CratestackRequestConfig,
  type CratestackWriteRequestConfig,
} from "./queries.js";

export class TinyRestClientClient {
  readonly runtime: CratestackRuntime;
  readonly procedures: ProceduresApi;
  readonly widgets: WidgetApi;

  constructor(originOrRuntime: string | CratestackRuntime, options: CratestackClientOptions = {}) {
    this.runtime = typeof originOrRuntime === "string"
      ? new CratestackRuntime(originOrRuntime, options)
      : originOrRuntime;
    this.procedures = new ProceduresApi(this.runtime);
    this.widgets = new WidgetApi(this.runtime);
  }
}

export class WidgetApi {
  constructor(private readonly runtime: CratestackRuntime) {}

  list(options: CratestackQueryRequestConfig = {}): Promise<Widget[]> {
    return this.runtime.get<unknown>("/widgets", {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((value) => reviveDecimalFields(value, 'Widget') as Widget[]);
  }

  get(id: number, options: CratestackQueryRequestConfig = {}): Promise<Widget> {
    return this.runtime.get<unknown>(`/widgets/${encodeURIComponent(String(id))}`, {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((value) => reviveDecimalFields(value, 'Widget') as Widget);
  }

  // Same call as `get`, but returns the response alongside the record
  // (issue #610) — read `.response.headers.get("etag")` off the result
  // to get the value `update`/`delete`'s `ifMatch` option needs.
  getWithResponse(
    id: number,
    options: CratestackQueryRequestConfig = {},
  ): Promise<CratestackResponseEnvelope<Widget>> {
    return this.runtime.getWithResponse<unknown>(`/widgets/${encodeURIComponent(String(id))}`, {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    }).then((result) => ({
      value: reviveDecimalFields(result.value, 'Widget') as Widget,
      response: result.response,
    }));
  }

  create(input: CreateWidgetInput, options: CratestackRequestConfig = {}): Promise<Widget> {
    return this.runtime.post<unknown>("/widgets", input, options)
      .then((value) => reviveDecimalFields(value, 'Widget') as Widget);
  }

  update(
    id: number,
    input: UpdateWidgetInput,
    options: CratestackWriteRequestConfig = {},
  ): Promise<Widget> {
    return this.runtime.patch<unknown>(`/widgets/${encodeURIComponent(String(id))}`, input, {
      headers: withIfMatchHeader(options.headers, options.ifMatch),
      signal: options.signal,
    })
      .then((value) => reviveDecimalFields(value, 'Widget') as Widget);
  }

  delete(id: number, options: CratestackWriteRequestConfig = {}): Promise<void> {
    return this.runtime.delete<void>(`/widgets/${encodeURIComponent(String(id))}`, {
      headers: withIfMatchHeader(options.headers, options.ifMatch),
      signal: options.signal,
    });
  }
}

export class ProceduresApi {
  constructor(private readonly runtime: CratestackRuntime) {}

  echoName(args: EchoNameArgs, options: CratestackRequestConfig = {}): Promise<string> {
    return this.runtime.post<unknown>("/$procs/echoName", args, options)
      .then((value) => reviveDecimalFields(value, 'String') as string);
  }

}