// `runtime.batch()` and `@cratestack/link-batch` on the real
// `@cratestack/cbor`: each frame's `output` carries the server's 16-byte
// `Uuid`, and each frame's `input` still carries text.
import { DocApi, ProceduresApi } from "./src/client.js";
import { CratestackRpcRuntime } from "./src/runtime.js";
import { createBatchLink } from "./link-batch/index.js";
import { TEXT, TEXT2, check, cborResponse, realCodec, uuidBytes } from "./common.mjs";

const codec = await realCodec();

type Frame = { id: number; op: string; input: Record<string, unknown> };

// 1. runtime.batch(): raw results come back revived per op.
{
  const fetchFn: typeof fetch = async () =>
    cborResponse([
      { id: 1, output: { id: uuidBytes(TEXT), ownerId: uuidBytes(TEXT2), parentId: null, tagIds: [], title: "t" } },
      { id: 2, output: uuidBytes(TEXT2) },
    ]);
  const runtime = new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn });
  const results = (await runtime.batch([
    { id: 1, op: "model.Doc.get", input: { id: TEXT } },
    { id: 2, op: "procedure.echoId", input: { id: TEXT2 } },
  ] as never)) as unknown as Array<{ output?: unknown }>;
  check(results.length === 2, `expected 2 results, got ${results.length}`);
}

// 2. link-batch: two typed calls in one flush.
{
  const bodies: Uint8Array[] = [];
  const fetchFn: typeof fetch = async (_url, init) => {
    bodies.push(init?.body as Uint8Array);
    const frames = codec.decode(init?.body as Uint8Array) as Frame[];
    return cborResponse(
      frames.map((frame) => ({
        id: frame.id,
        output:
          frame.op === "model.Doc.get"
            ? { id: uuidBytes(TEXT), ownerId: uuidBytes(TEXT2), parentId: null, tagIds: [uuidBytes(TEXT)], title: "t" }
            : uuidBytes(TEXT2),
      })),
    );
  };
  const runtime = new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn, links: [createBatchLink()] });
  const [doc, echoed] = await Promise.all([
    new DocApi(runtime).get(TEXT),
    new ProceduresApi(runtime).echoId({ id: TEXT2 }),
  ]);
  check(bodies.length === 1, `expected one batched request, got ${bodies.length}`);
  const frames = codec.decode(bodies[0]!) as Frame[];
  check(frames.length === 2, `expected two frames, got ${frames.length}`);
  check(frames[0]!.input.id === TEXT, `get frame carried ${typeof frames[0]!.input.id}, want the string`);
  check(frames[1]!.input.id === TEXT2, `echoId frame carried ${typeof frames[1]!.input.id}, want the string`);
  check(doc.id === TEXT && doc.ownerId === TEXT2 && doc.tagIds[0] === TEXT, `batched get() returned ${JSON.stringify(doc)}`);
  check(echoed === TEXT2, `batched echoId() returned ${String(echoed)}`);
}

console.log("NATIVE_CBOR_UUID_BATCH_OK");
