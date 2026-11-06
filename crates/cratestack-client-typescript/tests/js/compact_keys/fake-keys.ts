// A stand-in for `@cratestack/cbor`'s `createCompactKeys`, so the generated
// runtimes' negotiation is provable without the native binding. The "compact"
// form is structural on purpose (a CBOR byte string holding the named bytes,
// a JSON object holding the named text) so the real cbor-seq boundary scanner
// can walk compact items.
export interface FakeKeys {
  calls: string[];
  tables: Uint8Array[];
  selectorFor: (op: string) => string | undefined;
}

export function newFakeKeys(): FakeKeys {
  return { calls: [], tables: [], selectorFor: (op) => `sel-${op}` };
}

type CreateCborCodec = () => Promise<{
  encode(value: unknown): BodyInit;
  decode(bytes: Uint8Array): unknown;
}>;

/** The `createCompactKeys` a test mocks `@cratestack/cbor` with. */
export function makeFakeCreate(state: FakeKeys, createCborCodec: CreateCborCodec) {
  return async (tables: Uint8Array) => {
    state.tables.push(tables);
    const codec = await createCborCodec();
    return fakeKeys(state, codec);
  };
}

function fakeKeys(
  state: FakeKeys,
  codec: { encode(value: unknown): BodyInit; decode(bytes: Uint8Array): unknown },
) {
  const bytes = (value: BodyInit): Uint8Array => value as Uint8Array;
  return {
    toCompact(named: Uint8Array, op: string, direction: string): Uint8Array {
      state.calls.push(`toCompact:${op}:${direction}`);
      return bytes(codec.encode(named)) as Uint8Array;
    },
    fromCompact(compact: Uint8Array, op: string, direction: string): Uint8Array {
      state.calls.push(`fromCompact:${op}:${direction}`);
      return codec.decode(compact) as Uint8Array;
    },
    toCompactJson(named: string, op: string, direction: string): string {
      state.calls.push(`toCompactJson:${op}:${direction}`);
      return JSON.stringify({ c: named });
    },
    fromCompactJson(compact: string, op: string, direction: string): string {
      state.calls.push(`fromCompactJson:${op}:${direction}`);
      return (JSON.parse(compact) as { c: string }).c;
    },
    selector(op: string): string | undefined {
      return state.selectorFor(op);
    },
  };
}
