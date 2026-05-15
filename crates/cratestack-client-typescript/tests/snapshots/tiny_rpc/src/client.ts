import {
  CratestackRpcRuntime,
  type CratestackRpcCallOptions,
  type CratestackRpcClientOptions,
} from "./runtime";
import type {
  Widget,
  CreateWidgetInput,
  UpdateWidgetInput,
  EchoNameArgs,
  Page,
} from "./models";

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

  list(input: Record<string, unknown> = {}, options: CratestackRpcCallOptions = {}): Promise<Widget[]> {
    return this.runtime.call<Record<string, unknown>, Widget[]>(
      "model.Widget.list",
      input,
      options,
    );
  }

  get(id: number, options: CratestackRpcCallOptions = {}): Promise<Widget> {
    return this.runtime.call<{ id: number }, Widget>(
      "model.Widget.get",
      { id },
      options,
    );
  }

  create(input: CreateWidgetInput, options: CratestackRpcCallOptions = {}): Promise<Widget> {
    return this.runtime.call<CreateWidgetInput, Widget>(
      "model.Widget.create",
      input,
      options,
    );
  }

  update(
    id: number,
    patch: UpdateWidgetInput,
    options: CratestackRpcCallOptions = {},
  ): Promise<Widget> {
    return this.runtime.call<{ id: number; patch: UpdateWidgetInput }, Widget>(
      "model.Widget.update",
      { id, patch },
      options,
    );
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