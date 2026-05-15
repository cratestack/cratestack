import { CratestackRuntime, type CratestackClientOptions } from "./runtime";
import type {
  Widget,
  CreateWidgetInput,
  UpdateWidgetInput,
  EchoNameArgs,
  Page,
} from "./models";
import { toSearchQuery, type CratestackQueryRequestConfig, type CratestackRequestConfig } from "./queries";

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
    return this.runtime.get<Widget[]>("/widgets", {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    });
  }

  get(id: number, options: CratestackQueryRequestConfig = {}): Promise<Widget> {
    return this.runtime.get<Widget>(`/widgets/${encodeURIComponent(String(id))}`, {
      headers: options.headers,
      query: toSearchQuery(options.query),
      signal: options.signal,
    });
  }

  create(input: CreateWidgetInput, options: CratestackRequestConfig = {}): Promise<Widget> {
    return this.runtime.post<Widget>("/widgets", input, options);
  }

  update(
    id: number,
    input: UpdateWidgetInput,
    options: CratestackRequestConfig = {},
  ): Promise<Widget> {
    return this.runtime.patch<Widget>(`/widgets/${encodeURIComponent(String(id))}`, input, options);
  }

  delete(id: number, options: CratestackRequestConfig = {}): Promise<void> {
    return this.runtime.delete<void>(`/widgets/${encodeURIComponent(String(id))}`, options);
  }
}

export class ProceduresApi {
  constructor(private readonly runtime: CratestackRuntime) {}

  echoName(args: EchoNameArgs, options: CratestackRequestConfig = {}): Promise<string> {
    return this.runtime.post<string>("/$procs/echoName", args, options);
  }

}