// The `--swr` layout keeps its OWN copy of the revival runtime
// (`src/swr/models/shared.ts`, not `src/models.ts`), so it needs its own
// proof: a 16-byte `Uint8Array` at a `Uuid` key becomes the canonical string,
// at every arity, nested, and for a bare `Uuid` return; a wrong length throws.
import { reviveWireFields, reviveWireScalar } from "./src/swr/models/shared.js";
import { TEXT, TEXT2, check, uuidBytes } from "./common.mjs";

const doc = reviveWireFields(
  { id: uuidBytes(TEXT), ownerId: uuidBytes(TEXT2), parentId: null, tagIds: [uuidBytes(TEXT)], title: "t" },
  "Doc",
) as { id: unknown; ownerId: unknown; parentId: unknown; tagIds: unknown[]; title: unknown };
check(doc.id === TEXT && doc.ownerId === TEXT2, `field revival: ${JSON.stringify(doc)}`);
check(doc.parentId === null && doc.tagIds[0] === TEXT && doc.title === "t", `other arities: ${JSON.stringify(doc)}`);

const pair = reviveWireFields({ first: uuidBytes(TEXT), others: [uuidBytes(TEXT2)] }, "Pair") as {
  first: unknown;
  others: unknown[];
};
check(pair.first === TEXT && pair.others[0] === TEXT2, `type revival: ${JSON.stringify(pair)}`);

check(reviveWireScalar(uuidBytes(TEXT2), "uuid") === TEXT2, "bare Uuid return");
check(reviveWireScalar(TEXT2, "uuid") === TEXT2, "a string passes through");

try {
  reviveWireFields({ id: new Uint8Array(3) }, "Doc");
  throw new Error("a 3-byte Uuid was accepted");
} catch (error) {
  check(error instanceof TypeError, `expected a TypeError, got ${String(error)}`);
  check(/Uuid field Doc\.id: expected a UUID string or 16 bytes, got 3 bytes/.test(error.message), error.message);
}

console.log("NATIVE_CBOR_UUID_SWR_OK");
