// Generated procedures module for the `swr` preset (issue #304): every
// procedure's own `<Name>Args` wrapper type plus a plain, framework-free
// `async` function per procedure. No client class, no React import —
// every function takes a `CratestackRpcRuntime` as its first argument.
//
// A procedure's own `<Name>Args` type is always defined here — it's
// already scoped to its one procedure
// (`crate::naming::procedure_wrapper_name`), never shared. Enums/`type`
// blocks referenced by exactly this file (not by any model) are inlined
// below too; anything referenced by 2+ consumers (or by a model as well)
// lives in `./models/shared` and is imported instead — see
// `cratestack-client-typescript`'s
// `src/swr/ownership.rs::compute_type_ownership`.

import type { CratestackRpcRuntime, CratestackRpcCallOptions } from "./runtime.js";
// cratestack#498: see `procedures-rest.ts.j2`'s identical import for why
// this is a real (not type-only) import.
import { reviveWireFields, revivePagedWireFields, reviveWireScalar } from "./models/shared.js";

export interface EchoNameArgs {
  name: string;
}

export async function echoName(
  runtime: CratestackRpcRuntime,
  args: EchoNameArgs,
  options: CratestackRpcCallOptions = {},
): Promise<string> {
  return runtime.call<EchoNameArgs, unknown>(
    "procedure.echoName",
    args,
    options,
  ).then((value) => reviveWireFields(value, 'String') as string);
}

