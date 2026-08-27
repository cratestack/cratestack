import {
  CratestackRpcRuntime,
  type CratestackRpcCallOptions,
  type CratestackRpcClientOptions,
} from "./runtime.js";
import { toRpcListInput, type CratestackRpcListQuery } from "./queries.js";
import { reviveWireFields, revivePagedWireFields, reviveWireScalar } from "./models.js";
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

export class TinyRpcNativeDefaultClientClient {
  readonly runtime: CratestackRpcRuntime;
  readonly procedures: ProceduresApi;
  readonly widgets: WidgetApi;

  constructor(originOrRuntime: string | CratestackRpcRuntime, options: CratestackRpcClientOptions = {}) {
    this.runtime = typeof originOrRuntime === "string"
      ? new CratestackRpcRuntime(originOrRuntime, options)
      : originOrRuntime;
    this.procedures = new ProceduresApi(this.runtime);
    this.widgets = new WidgetApi(this.runtime);
  }
}

export interface WidgetApiGetOptions extends CratestackRpcCallOptions {
  fields?: string[];
  include?: string[];
  includeFields?: Record<string, string[]>;
}

export class WidgetApi {
  constructor(private readonly runtime: CratestackRpcRuntime) {}

  list(query: CratestackRpcListQuery = {}, options: CratestackRpcCallOptions = {}): Promise<Widget[]> {
    return this.runtime.call<Record<string, unknown>, unknown>(
      "model.Widget.list",
      toRpcListInput(query),
      options,
    ).then((value) => reviveWireFields(value, 'Widget') as Widget[]);
  }

  get(id: number, options: WidgetApiGetOptions = {}): Promise<Widget> {
    const input: Record<string, unknown> = { id };
    if (options.fields?.length) {
      input.fields = options.fields;
    }
    if (options.include?.length) {
      input.include = options.include;
    }
    if (options.includeFields && Object.keys(options.includeFields).length > 0) {
      // snake_case on purpose: mirrors toRpcListInput and RpcGetInput.include_fields,
      // which carry no camelCase rename on the wire.
      input.include_fields = options.includeFields;
    }
    return this.runtime.call<Record<string, unknown>, unknown>(
      "model.Widget.get",
      input,
      options,
    ).then((value) => reviveWireFields(value, 'Widget') as Widget);
  }

  create(input: CreateWidgetInput, options: CratestackRpcCallOptions = {}): Promise<Widget> {
    return this.runtime.call<CreateWidgetInput, unknown>(
      "model.Widget.create",
      input,
      options,
    ).then((value) => reviveWireFields(value, 'Widget') as Widget);
  }

  update(
    id: number,
    patch: UpdateWidgetInput,
    options: CratestackRpcCallOptions = {},
  ): Promise<Widget> {
    return this.runtime.call<{ id: number; patch: UpdateWidgetInput }, unknown>(
      "model.Widget.update",
      { id, patch },
      options,
    ).then((value) => reviveWireFields(value, 'Widget') as Widget);
  }

  delete(id: number, options: CratestackRpcCallOptions = {}): Promise<void> {
    return this.runtime.call<{ id: number }, void>(
      "model.Widget.delete",
      { id },
      options,
    );
  }
}

export class ProceduresApi {
  constructor(private readonly runtime: CratestackRpcRuntime) {}

  echoName(args: EchoNameArgs, options: CratestackRpcCallOptions = {}): Promise<string> {
    return this.runtime.call<EchoNameArgs, unknown>(
      "procedure.echoName",
      args,
      options,
    ).then((value) => reviveWireFields(value, 'String') as string);
  }

}