import {
  CratestackGrpcWebRuntime,
  type CratestackGrpcCallOptions,
  type CratestackGrpcClientOptions,
  type GrpcFieldDescriptor,
} from "./runtime";
import type {
  Widget,
  CreateWidgetInput,
  UpdateWidgetInput,
  Page,
} from "./models";

// Field-number tables sourced from the schema's `.pb.lock` at generation
// time (`docs/design/protobuf.md` §3.3) — the same numbers the Rust
// server's mirror structs and `.proto` artifact use, so this client's
// wire bytes decode correctly on the real server.
//
// Deliberately NOT typed as `GrpcMessageRegistry` here: the generated
// tsconfig turns on `noUncheckedIndexedAccess`, which would widen every
// `MESSAGES.Widget`-style access below to `... | undefined` if this were
// annotated with the `Record<string, ...>` runtime type. Left as an
// inferred object literal, each key keeps its own exact type, and the
// whole object still satisfies `GrpcMessageRegistry` structurally at the
// `runtime.unary(...)` call sites that need the wider type.
const MESSAGES = {
  CreateWidgetInput: [
    { property: "id", number: 1, kind: "int64", repeated: false },
    { property: "name", number: 2, kind: "string", repeated: false },
  ] satisfies GrpcFieldDescriptor[],
  PageInfo: [
    { property: "limit", number: 1, kind: "int64", repeated: false },
    { property: "offset", number: 2, kind: "int64", repeated: false },
    { property: "hasNextPage", number: 3, kind: "bool", repeated: false, defaultsWhenAbsent: true },
    { property: "hasPreviousPage", number: 4, kind: "bool", repeated: false, defaultsWhenAbsent: true },
  ] satisfies GrpcFieldDescriptor[],
  PageOfWidget: [
    { property: "items", number: 1, kind: "message", repeated: true, refName: "Widget" },
    { property: "totalCount", number: 2, kind: "int64", repeated: false },
    { property: "pageInfo", number: 3, kind: "message", repeated: false, refName: "PageInfo" },
  ] satisfies GrpcFieldDescriptor[],
  UpdateWidgetInput: [
    { property: "name", number: 1, kind: "string", repeated: false },
  ] satisfies GrpcFieldDescriptor[],
  Widget: [
    { property: "id", number: 1, kind: "int64", repeated: false },
    { property: "name", number: 2, kind: "string", repeated: false },
  ] satisfies GrpcFieldDescriptor[],
  WidgetRpcListInput: [
    { property: "limit", number: 1, kind: "int64", repeated: false },
    { property: "offset", number: 2, kind: "int64", repeated: false },
    { property: "fields", number: 3, kind: "string", repeated: true },
    { property: "include", number: 4, kind: "string", repeated: true },
    { property: "sort", number: 6, kind: "string", repeated: false },
  ] satisfies GrpcFieldDescriptor[],
  WidgetRpcPkInput: [
    { property: "id", number: 1, kind: "int64", repeated: false },
  ] satisfies GrpcFieldDescriptor[],
  WidgetRpcUpdateInput: [
    { property: "id", number: 1, kind: "int64", repeated: false },
    { property: "patch", number: 2, kind: "message", repeated: false, refName: "UpdateWidgetInput" },
  ] satisfies GrpcFieldDescriptor[],
};

const ENUMS = {
};

export class CratestackExamplesWidgetsGrpcClientClient {
  readonly runtime: CratestackGrpcWebRuntime;
  readonly widgets: WidgetApi;

  constructor(originOrRuntime: string | CratestackGrpcWebRuntime, options: CratestackGrpcClientOptions = {}) {
    this.runtime = typeof originOrRuntime === "string"
      ? new CratestackGrpcWebRuntime(originOrRuntime, options)
      : originOrRuntime;
    this.widgets = new WidgetApi(this.runtime);
  }
}

/** `list()`'s input covers the common list-projection controls
 *  (`limit`/`offset`/`fields`/`include`/`sort`) — raw predicate queries
 *  (`where`/`or`/structured filters) and per-relation field projection
 *  (`includeFields`) are not wired into the generated gRPC-Web client in
 *  this pass; see this ticket's final report. */
export interface GrpcListInput {
  limit?: number;
  offset?: number;
  fields?: string[];
  include?: string[];
  sort?: string;
}

export class WidgetApi {
  constructor(private readonly runtime: CratestackGrpcWebRuntime) {}

  async list(input: GrpcListInput = {}, options: CratestackGrpcCallOptions = {}): Promise<Page<Widget>> {
    const result = await this.runtime.unary(
      "/widgets_api.Api/ModelWidgetList",
      input as Record<string, unknown>,
      MESSAGES.WidgetRpcListInput,
      MESSAGES.PageOfWidget,
      MESSAGES,
      ENUMS,
      options,
    );
    return result as unknown as Page<Widget>;
  }

  async get(id: number, options: CratestackGrpcCallOptions = {}): Promise<Widget> {
    const result = await this.runtime.unary(
      "/widgets_api.Api/ModelWidgetGet",
      { id },
      MESSAGES.WidgetRpcPkInput,
      MESSAGES.Widget,
      MESSAGES,
      ENUMS,
      options,
    );
    return result as unknown as Widget;
  }

  async create(input: CreateWidgetInput, options: CratestackGrpcCallOptions = {}): Promise<Widget> {
    const result = await this.runtime.unary(
      "/widgets_api.Api/ModelWidgetCreate",
      input as unknown as Record<string, unknown>,
      MESSAGES.CreateWidgetInput,
      MESSAGES.Widget,
      MESSAGES,
      ENUMS,
      options,
    );
    return result as unknown as Widget;
  }

  async update(
    id: number,
    patch: UpdateWidgetInput,
    options: CratestackGrpcCallOptions = {},
  ): Promise<Widget> {
    const result = await this.runtime.unary(
      "/widgets_api.Api/ModelWidgetUpdate",
      { id, patch },
      MESSAGES.WidgetRpcUpdateInput,
      MESSAGES.Widget,
      MESSAGES,
      ENUMS,
      options,
    );
    return result as unknown as Widget;
  }

  async delete(id: number, options: CratestackGrpcCallOptions = {}): Promise<void> {
    await this.runtime.unary(
      "/widgets_api.Api/ModelWidgetDelete",
      { id },
      MESSAGES.WidgetRpcPkInput,
      MESSAGES.Widget,
      MESSAGES,
      ENUMS,
      options,
    );
  }
}

