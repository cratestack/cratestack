/** A tiny in-memory RPC server, injected as the generated RPC client's own
 *  `fetch` (`CratestackRpcClientOptions.fetch` — `rpc-runtime.ts.j2`), so
 *  every test in the RPC suite drives the REAL generated
 *  `RefineFixtureRpcClientClient` (URL building, JSON codec encode/decode,
 *  header threading, error mapping) rather than a hand-written stand-in
 *  for it. Only the network boundary is faked, same shape as
 *  `fake-rest-server.ts`'s `FakeRestServer` for REST.
 *
 *  Speaks `POST /rpc/{op_id}` with a JSON body (the runtime's default
 *  `jsonRpcCodec`) — `model.<M>.list`/`.get`/`.create`/`.update`/`.delete`,
 *  matching the wire shapes `crates/cratestack-macros/src/transport/rpc.rs`
 *  dispatches: `RpcListInput`-shaped bodies for list, `{ id }` for
 *  get/delete, the raw create input for create, `{ id, patch }` for
 *  update. Implements the same decisive semantics as the REST fake:
 *  `field__operator` filtering + `-field` sort (via `./list-match.js`,
 *  shared with the REST fake so both provably parse the identical DSL),
 *  `limit`/`offset` slicing with a real `totalCount`, and `If-Match`
 *  optimistic locking on update/delete (missing or stale -> 412 with an
 *  `RpcErrorBody`, matching cratestack#493/#519/#538 — see
 *  `crates/cratestack-macros/src/axum/model/prep/etag.rs` and
 *  `crates/cratestack-macros/src/transport/rpc.rs`'s confirmation that
 *  the real HTTP `HeaderMap` reaches the identical dispatch fn REST
 *  uses). */

import { applyFilterPairs, applySort } from "./list-match.js";

export interface FakeRpcResourceSchema {
  /** The schema model name, as it appears in the `model.<M>.<verb>` op id
   *  (e.g. `"Widget"`) — NOT the client's camelCase/pluralized accessor. */
  model: string;
  primaryKey: string;
  versionField?: string;
  paged?: boolean;
}

interface RpcErrorBody {
  code: string;
  message: string;
}

type Row = Record<string, unknown>;

export class FakeRpcServer {
  private readonly schemas = new Map<string, FakeRpcResourceSchema>();
  private readonly tables = new Map<string, Map<string, Row>>();
  readonly requests: { opId: string; url: string; headers: Headers; body: unknown }[] = [];

  constructor(schemas: FakeRpcResourceSchema[]) {
    for (const schema of schemas) {
      this.schemas.set(schema.model, schema);
      this.tables.set(schema.model, new Map());
    }
  }

  seed(model: string, record: Row): void {
    const schema = this.mustSchema(model);
    this.tables.get(model)!.set(String(record[schema.primaryKey]), { ...record });
  }

  row(model: string, id: unknown): Row | undefined {
    return this.tables.get(model)!.get(String(id));
  }

  readonly fetch: typeof fetch = async (input, init = {}) => {
    const url = new URL(typeof input === "string" ? input : (input as URL | Request).toString());
    const headers = new Headers(init.headers);
    const opId = decodeURIComponent(url.pathname.replace(/^.*\/rpc\//, ""));
    const body: unknown = init.body ? JSON.parse(init.body as string) : null;
    this.requests.push({ opId, url: url.toString(), headers, body });

    const match = /^model\.([^.]+)\.(list|get|create|update|delete)$/.exec(opId);
    if (!match) {
      return rpcError(404, "not_found", `no rpc op matches ${opId}`);
    }
    const [, model, verb] = match as [
      string,
      string,
      "list" | "get" | "create" | "update" | "delete",
    ];
    const schema = this.schemas.get(model!);
    if (!schema) {
      return rpcError(404, "not_found", `no resource registered for model ${model}`);
    }
    const table = this.tables.get(model!)!;
    const input_ = (body ?? {}) as Row;

    switch (verb) {
      case "list":
        return this.list(schema, table, input_);
      case "get":
        return this.getOne(table, input_.id);
      case "create":
        return this.create(schema, table, input_);
      case "update":
        return this.update(schema, table, input_.id, input_.patch as Row, headers);
      case "delete":
        return this.delete(schema, table, input_.id, headers);
      default:
        return rpcError(404, "not_found", `unsupported verb ${verb}`);
    }
  };

  private mustSchema(model: string): FakeRpcResourceSchema {
    const schema = this.schemas.get(model);
    if (!schema) throw new Error(`fake rpc server: no resource registered for model "${model}"`);
    return schema;
  }

  private list(schema: FakeRpcResourceSchema, table: Map<string, Row>, input: Row): Response {
    let items = applySort([...table.values()], input.sort as string | undefined);
    const filters = (input.filters as { key: string; value: string }[] | undefined) ?? [];
    items = applyFilterPairs(
      items,
      filters.map((f) => [f.key, f.value] as [string, string]),
    );
    const total = items.length;
    const limit = typeof input.limit === "number" ? input.limit : undefined;
    const offset = typeof input.offset === "number" ? input.offset : 0;
    const page = limit === undefined ? items : items.slice(offset, offset + limit);

    if (schema.paged) {
      return jsonResponse(200, {
        items: page,
        totalCount: total,
        pageInfo: {
          limit: limit ?? null,
          offset,
          hasNextPage: offset + page.length < total,
          hasPreviousPage: offset > 0,
        },
      });
    }
    return jsonResponse(200, page);
  }

  private getOne(table: Map<string, Row>, id: unknown): Response {
    const record = table.get(String(id));
    return record ? jsonResponse(200, record) : rpcError(404, "not_found", "not found");
  }

  private create(schema: FakeRpcResourceSchema, table: Map<string, Row>, body: Row): Response {
    const pk = String(body[schema.primaryKey]);
    if (table.has(pk)) return rpcError(409, "conflict", "already exists");
    const record: Row = { ...body };
    if (schema.versionField) record[schema.versionField] = 1;
    table.set(pk, record);
    return jsonResponse(200, record);
  }

  private update(
    schema: FakeRpcResourceSchema,
    table: Map<string, Row>,
    id: unknown,
    patch: Row,
    headers: Headers,
  ): Response {
    const record = table.get(String(id));
    if (!record) return rpcError(404, "not_found", "not found");
    const conflict = checkIfMatch(schema, record, headers);
    if (conflict) return conflict;
    const updated: Row = { ...record, ...patch };
    if (schema.versionField)
      updated[schema.versionField] = (record[schema.versionField] as number) + 1;
    table.set(String(id), updated);
    return jsonResponse(200, updated);
  }

  private delete(
    schema: FakeRpcResourceSchema,
    table: Map<string, Row>,
    id: unknown,
    headers: Headers,
  ): Response {
    const record = table.get(String(id));
    if (!record) return rpcError(404, "not_found", "not found");
    const conflict = checkIfMatch(schema, record, headers);
    if (conflict) return conflict;
    table.delete(String(id));
    return new Response(null, { status: 204 });
  }
}

function checkIfMatch(
  schema: FakeRpcResourceSchema,
  record: Row,
  headers: Headers,
): Response | undefined {
  if (!schema.versionField) return undefined;
  const expected = `"${record[schema.versionField]}"`;
  const ifMatch = headers.get("If-Match");
  if (ifMatch !== expected) {
    return rpcError(
      412,
      "failed_precondition",
      `expected If-Match: ${expected}, got: ${ifMatch ?? "<none>"}`,
    );
  }
  return undefined;
}

function jsonResponse(status: number, body: unknown): Response {
  return new Response(status === 204 ? null : JSON.stringify(body), {
    status,
    headers: status === 204 ? undefined : { "Content-Type": "application/json" },
  });
}

function rpcError(status: number, code: string, message: string): Response {
  const body: RpcErrorBody = { code, message };
  return jsonResponse(status, body);
}
