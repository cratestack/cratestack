/** A tiny in-memory REST server, injected as the generated client's own
 *  `fetch` (`CratestackClientOptions.fetch` — `runtime.ts.j2`), so every
 *  test in this suite drives the REAL generated `RefineFixtureClientClient`
 *  (URL building, header threading, JSON encode/decode, error mapping)
 *  rather than a hand-written stand-in for it. Only the network boundary
 *  is faked — same shape as `crates/cratestack-client-typescript`'s own
 *  `FakeRuntime` test harness pattern, and `packages/cratestack-link-batch`'s
 *  `fakeBatchFetch`.
 *
 *  Implements just enough server semantics to make the decisive tests
 *  meaningful: `field__operator=value` filtering, `sort`, `limit`/
 *  `offset` slicing with a real `totalCount`, and `If-Match` optimistic
 *  locking on PATCH/DELETE (missing or stale -> 412, matching cratestack#493/#519/#538). */

export interface FakeResourceSchema {
  path: string;
  primaryKey: string;
  versionField?: string;
  paged?: boolean;
}

type Row = Record<string, unknown>;

export class FakeRestServer {
  private readonly schemas = new Map<string, FakeResourceSchema>();
  private readonly tables = new Map<string, Map<string, Row>>();
  readonly requests: { method: string; url: string; headers: Headers }[] = [];

  constructor(schemas: FakeResourceSchema[]) {
    for (const schema of schemas) {
      this.schemas.set(schema.path, schema);
      this.tables.set(schema.path, new Map());
    }
  }

  seed(path: string, record: Row): void {
    const schema = this.mustSchema(path);
    this.tables.get(path)!.set(String(record[schema.primaryKey]), { ...record });
  }

  row(path: string, id: unknown): Row | undefined {
    return this.tables.get(path)!.get(String(id));
  }

  readonly fetch: typeof fetch = async (input, init = {}) => {
    const url = new URL(typeof input === "string" ? input : (input as URL | Request).toString());
    const method = (init.method ?? "GET").toUpperCase();
    const headers = new Headers(init.headers);
    this.requests.push({ method, url: url.toString(), headers });

    const segments = url.pathname.replace(/^\/+/, "").split("/");
    const resourceAt = segments.findIndex((segment) => this.schemas.has(segment));
    if (resourceAt === -1) {
      return jsonResponse(404, {
        code: "NOT_FOUND",
        message: `no resource matches ${url.pathname}`,
      });
    }
    const schema = this.schemas.get(segments[resourceAt]!)!;
    const table = this.tables.get(schema.path)!;
    const id = segments[resourceAt + 1];
    const body = init.body ? (JSON.parse(init.body as string) as Row) : undefined;

    if (method === "GET" && id === undefined) return this.list(schema, table, url.searchParams);
    if (method === "GET") return this.getOne(table, id!);
    if (method === "POST") return this.create(schema, table, body!);
    if (method === "PATCH") return this.update(schema, table, id!, body!, headers);
    if (method === "DELETE") return this.delete(schema, table, id!, headers);
    return jsonResponse(405, { code: "METHOD_NOT_ALLOWED", message: method });
  };

  private mustSchema(path: string): FakeResourceSchema {
    const schema = this.schemas.get(path);
    if (!schema) throw new Error(`fake server: no resource registered for "${path}"`);
    return schema;
  }

  private list(
    schema: FakeResourceSchema,
    table: Map<string, Row>,
    params: URLSearchParams,
  ): Response {
    let items = applySort([...table.values()], params.get("sort"));
    items = applyFilters(items, params);
    const total = items.length;
    const limit = params.has("limit") ? Number(params.get("limit")) : undefined;
    const offset = params.has("offset") ? Number(params.get("offset")) : 0;
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

  private getOne(table: Map<string, Row>, id: string): Response {
    const record = table.get(id);
    return record
      ? jsonResponse(200, record)
      : jsonResponse(404, { code: "NOT_FOUND", message: "not found" });
  }

  private create(schema: FakeResourceSchema, table: Map<string, Row>, body: Row): Response {
    const pk = String(body[schema.primaryKey]);
    if (table.has(pk)) return jsonResponse(409, { code: "CONFLICT", message: "already exists" });
    const record: Row = { ...body };
    if (schema.versionField) record[schema.versionField] = 1;
    table.set(pk, record);
    return jsonResponse(201, record);
  }

  private update(
    schema: FakeResourceSchema,
    table: Map<string, Row>,
    id: string,
    body: Row,
    headers: Headers,
  ): Response {
    const record = table.get(id);
    if (!record) return jsonResponse(404, { code: "NOT_FOUND", message: "not found" });
    const conflict = checkIfMatch(schema, record, headers);
    if (conflict) return conflict;
    const updated: Row = { ...record, ...body };
    if (schema.versionField)
      updated[schema.versionField] = (record[schema.versionField] as number) + 1;
    table.set(id, updated);
    return jsonResponse(200, updated);
  }

  private delete(
    schema: FakeResourceSchema,
    table: Map<string, Row>,
    id: string,
    headers: Headers,
  ): Response {
    const record = table.get(id);
    if (!record) return jsonResponse(404, { code: "NOT_FOUND", message: "not found" });
    const conflict = checkIfMatch(schema, record, headers);
    if (conflict) return conflict;
    table.delete(id);
    return new Response(null, { status: 204 });
  }
}

function checkIfMatch(
  schema: FakeResourceSchema,
  record: Row,
  headers: Headers,
): Response | undefined {
  if (!schema.versionField) return undefined;
  const expected = `"${record[schema.versionField]}"`;
  const ifMatch = headers.get("If-Match");
  if (ifMatch !== expected) {
    return jsonResponse(412, {
      code: "PRECONDITION_FAILED",
      message: `expected If-Match: ${expected}, got: ${ifMatch ?? "<none>"}`,
    });
  }
  return undefined;
}

const IGNORED_QUERY_KEYS = new Set(["limit", "offset", "sort", "fields", "include"]);

function applyFilters(items: Row[], params: URLSearchParams): Row[] {
  return items.filter((item) => {
    for (const [key, value] of params.entries()) {
      if (IGNORED_QUERY_KEYS.has(key)) continue;
      const separator = key.indexOf("__");
      const field = separator === -1 ? key : key.slice(0, separator);
      const operator = separator === -1 ? "eq" : key.slice(separator + 2);
      if (!matches(item[field], operator, value)) return false;
    }
    return true;
  });
}

function matches(actual: unknown, operator: string, value: string): boolean {
  switch (operator) {
    case "eq":
      return String(actual) === value;
    case "ne":
      return String(actual) !== value;
    case "in":
      return value.split(",").includes(String(actual));
    case "lt":
      return Number(actual) < Number(value);
    case "lte":
      return Number(actual) <= Number(value);
    case "gt":
      return Number(actual) > Number(value);
    case "gte":
      return Number(actual) >= Number(value);
    case "contains":
      return String(actual).includes(value);
    case "startsWith":
      return String(actual).startsWith(value);
    case "isNull":
      return String(actual === null || actual === undefined) === value;
    default:
      return true;
  }
}

function applySort(items: Row[], sortParam: string | null): Row[] {
  if (!sortParam) return items;
  const fields = sortParam.split(",");
  return [...items].sort((a, b) => {
    for (const raw of fields) {
      const desc = raw.startsWith("-");
      const field = desc ? raw.slice(1) : raw;
      const left = a[field] as number | string;
      const right = b[field] as number | string;
      if (left < right) return desc ? 1 : -1;
      if (left > right) return desc ? -1 : 1;
    }
    return 0;
  });
}

function jsonResponse(status: number, body: unknown): Response {
  return new Response(status === 204 ? null : JSON.stringify(body), {
    status,
    headers: status === 204 ? undefined : { "Content-Type": "application/json" },
  });
}
