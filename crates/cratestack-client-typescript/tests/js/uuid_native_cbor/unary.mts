// A generated RPC client on the real `@cratestack/cbor`: a `Uuid` the server
// wrote as 16 bytes decodes to the canonical string at every position (field,
// nullable, list, nested `type`, bare return), a string passes through (JSON
// shape, older server), a malformed value throws, and requests keep sending
// the text form.
import { DocApi, ProceduresApi } from "./src/client.js";
import { CratestackRpcRuntime } from "./src/runtime.js";
import { TEXT, TEXT2, check, cborResponse, includesBytes, realCodec, textStringBytes, uuidBytes } from "./common.mjs";

const codec = await realCodec();

function capturing(respond: () => Promise<Response>) {
  const sent: Uint8Array[] = [];
  const fetchFn: typeof fetch = async (_url, init) => {
    sent.push(init?.body as Uint8Array);
    return respond();
  };
  return { runtime: new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn }), sent };
}

// Model fields, all arities, from the server's byte-string form.
{
  const { runtime, sent } = capturing(() =>
    cborResponse({
      id: uuidBytes(TEXT),
      ownerId: uuidBytes(TEXT2),
      parentId: null,
      tagIds: [uuidBytes(TEXT), uuidBytes(TEXT2)],
      title: "t",
    }),
  );
  const doc = await new DocApi(runtime).get(TEXT);
  check(doc.id === TEXT, `id decoded to ${String(doc.id)}`);
  check(doc.ownerId === TEXT2, `ownerId decoded to ${String(doc.ownerId)}`);
  check(doc.parentId === null, `a null Uuid must stay null, got ${String(doc.parentId)}`);
  check(JSON.stringify(doc.tagIds) === JSON.stringify([TEXT, TEXT2]), `tagIds decoded to ${String(doc.tagIds)}`);

  // The request carried the id as a CBOR text string, not bytes.
  const request = codec.decode(sent[0]!) as { id: unknown };
  check(request.id === TEXT, `get() sent ${typeof request.id} ${String(request.id)}, want the string`);
  check(includesBytes(sent[0]!, await textStringBytes(TEXT)), "get() bytes lack the major-type-3 text string");
}

// An already-string value (JSON, or a server from before the byte-string form).
{
  const { runtime } = capturing(() =>
    cborResponse({ id: TEXT, ownerId: TEXT2, parentId: TEXT, tagIds: [], title: "t" }),
  );
  const doc = await new DocApi(runtime).get(TEXT);
  check(doc.id === TEXT && doc.parentId === TEXT && doc.tagIds.length === 0, "strings must pass through");
}

// Procedures: a bare Uuid return, and a nested `type` with a list.
{
  const { runtime, sent } = capturing(() => cborResponse(uuidBytes(TEXT2)));
  const result = await new ProceduresApi(runtime).echoId({ id: TEXT2 });
  check(result === TEXT2, `echoId() returned ${String(result)}`);
  const request = codec.decode(sent[0]!) as { id: unknown };
  check(request.id === TEXT2, `echoId() sent ${typeof request.id}, want the string`);
}
{
  const { runtime, sent } = capturing(() =>
    cborResponse({ first: uuidBytes(TEXT), others: [uuidBytes(TEXT2)] }),
  );
  const pair = await new ProceduresApi(runtime).echoPair({ input: { first: TEXT, others: [TEXT2] } });
  check(pair.first === TEXT && pair.others[0] === TEXT2, `echoPair() returned ${JSON.stringify(pair)}`);
  const request = codec.decode(sent[0]!) as { input: { first: unknown; others: unknown[] } };
  check(request.input.first === TEXT && request.input.others[0] === TEXT2, "echoPair() must send text");
}

// A wrong-length byte string is refused, naming the field, not passed through.
{
  const { runtime } = capturing(() =>
    cborResponse({ id: new Uint8Array(15), ownerId: uuidBytes(TEXT), parentId: null, tagIds: [], title: "t" }),
  );
  try {
    await new DocApi(runtime).get(TEXT);
    throw new Error("a 15-byte Uuid was accepted");
  } catch (error) {
    check(error instanceof TypeError, `expected a TypeError, got ${String(error)}`);
    check(
      /Uuid field Doc\.id: expected a UUID string or 16 bytes, got 15 bytes/.test(error.message),
      `unexpected message: ${error.message}`,
    );
  }
}

console.log("NATIVE_CBOR_UUID_UNARY_OK");
