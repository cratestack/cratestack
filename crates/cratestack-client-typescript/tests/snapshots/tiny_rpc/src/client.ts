import {
  CratestackRpcRuntime,
  type CratestackRpcCallOptions,
  type CratestackRpcClientOptions,
} from "./runtime.js";
import { toRpcListInput, type CratestackRpcListQuery } from "./queries.js";
import { reviveDecimalFields } from "./models.js";
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

export class TinyRpcClientClient {
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

export class WidgetApi {
  constructor(private readonly runtime: CratestackRpcRuntime) {}

  list(query: CratestackRpcListQuery = {}, options: CratestackRpcCallOptions = {}): Promise<Widget[]> {
    return this.runtime.call<Record<string, unknown>, unknown>(
      "model.Widget.list",
      toRpcListInput(query),
      options,
    ).then((value) => reviveDecimalFields(value, []) as Widget[]);
  }

  get(id: number, options: CratestackRpcCallOptions = {}): Promise<Widget> {
    return this.runtime.call<{ id: number }, unknown>(
      "model.Widget.get",
      { id },
      options,
    ).then((value) => reviveDecimalFields(value, []) as Widget);
  }

  create(input: CreateWidgetInput, options: CratestackRpcCallOptions = {}): Promise<Widget> {
    return this.runtime.call<CreateWidgetInput, unknown>(
      "model.Widget.create",
      input,
      options,
    ).then((value) => reviveDecimalFields(value, []) as Widget);
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
    ).then((value) => reviveDecimalFields(value, []) as Widget);
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
    return this.runtime.call<EchoNameArgs, string>(
      "procedure.echoName",
      args,
      options,
    );
  }

}