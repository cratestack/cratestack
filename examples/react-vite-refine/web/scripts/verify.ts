// Real end-to-end verification against a LIVE WireMock container — no
// React, no browser, driven straight through the generated client the
// same way `react-vite-swr`'s `scripts/seed.ts` proves its plain
// functions work outside React (issue #306 AC #4). Every request is
// logged; every response is asserted, not eyeballed — this script exits
// non-zero on the first unexpected result.
//
// Prerequisites (see README.md):
//   just react-vite-refine-fixture   # generates web/generated + wiremock/
//   docker build -t cratestack-wiremock-stateful \
//     -f ../../crates/cratestack-mock-wiremock/docker/Dockerfile \
//     ../../crates/cratestack-mock-wiremock/docker
//   docker run --rm -p 8080:8080 \
//     -v "$(pwd)/../wiremock/mappings:/home/wiremock/mappings:ro" \
//     cratestack-wiremock-stateful
//
// Run: pnpm run verify   (tsx scripts/verify.ts)
//
// Imports go straight to generated TS *source*, not the compiled
// package — same reason as `react-vite-swr`'s seed.ts (issue #315):
// the generated `dist/` output's relative imports have no `.js`
// extension, which Node's plain ESM resolver requires but `tsx`'s
// bundler-style resolution tolerates for source.
import { ReactViteRefineClientClient } from "../generated/src/client.ts";

const BASE_URL = process.env.CRATESTACK_API_URL ?? "http://localhost:8080";
let requestCount = 0;

const loggedFetch: typeof fetch = async (input, init = {}) => {
  requestCount += 1;
  const method = init.method ?? "GET";
  const url = typeof input === "string" ? input : input.toString();
  const headers = new Headers(init.headers);
  const headerPairs = [...headers.entries()].map(([k, v]) => `${k}: ${v}`).join(", ");
  console.log(`\n--> ${method} ${url}${headerPairs ? `  [${headerPairs}]` : ""}`);
  if (init.body) console.log(`    body: ${init.body}`);
  const response = await fetch(input, init);
  const clone = response.clone();
  const text = await clone.text();
  console.log(`<-- ${response.status} ${text}`);
  return response;
};

const client = new ReactViteRefineClientClient(BASE_URL, {
  basePath: "/api",
  fetch: loggedFetch,
});

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(`ASSERTION FAILED: ${message}`);
}

async function verifyCategory() {
  console.log("\n=== Category (plain CRUD) ===");
  // KNOWN, CONFIRMED MOCK BEHAVIOR (README.md "What this demo can't
  // prove"): `cratestack-mock-wiremock` ALWAYS server-generates the
  // primary key on create (`id_generator` in
  // `crates/cratestack-mock-wiremock/src/model_state/fragments.rs`) —
  // the submitted `id` below is accepted (satisfies the generated
  // `CreateCategoryInput` type, which a real `cratestack-pg` server DOES
  // honor) but silently ignored by THIS mock. Every follow-up call below
  // uses `created.id` — the id the mock actually assigned — never the
  // submitted one.
  const created = await client.categories.create({ id: 1, name: "Rust" });
  const id = created.id!;
  assert(created.name === "Rust", "create should echo the submitted name");

  const listed = await client.categories.list();
  assert(
    (listed as Array<{ id?: number }>).some((c) => c.id === id),
    "a created category must appear in a subsequent list",
  );

  const updated = await client.categories.update(id, { name: "Rust (updated)" });
  assert(updated.name === "Rust (updated)", "update should persist the new name");

  await client.categories.delete(id);
  const afterDelete = await client.categories.get(id).catch((error) => error);
  assert(
    afterDelete &&
      typeof afterDelete === "object" &&
      "status" in afterDelete &&
      afterDelete.status === 404,
    "get on a deleted record must 404, not return a stale body",
  );
  console.log("Category: create -> list -> update -> delete -> 404 all verified.");
}

async function verifyPostFalsyRoundTripAndIfMatchGap() {
  console.log("\n=== Post (@@paged + @version) ===");
  const created = await client.posts.create({
    id: 1,
    title: "Launch",
    published: false,
    version: 0,
  });
  const id = created.id!; // server-generated — see verifyCategory()'s comment
  assert(
    created.published === false,
    `cratestack#588: a falsy "published: false" must round-trip as false, got ${created.published}`,
  );

  const list = (await client.posts.list()) as {
    items: Array<{ id?: number }>;
    totalCount: number | null;
  };
  assert(Array.isArray(list.items), "@@paged list must return the { items, totalCount } envelope");
  assert(
    list.items.some((p) => p.id === id),
    "created post must appear in the paged list",
  );

  // The generated client always sends `If-Match` on a versioned model's
  // update once a version is known — but this script calls the model API
  // directly (bypassing @cratestack/refine's version cache), so it must
  // supply it explicitly, same as any direct API consumer would.
  const firstUpdate = await client.posts.update(
    id,
    { title: "Launch (v2)" },
    { headers: { "If-Match": '"0"' } },
  );
  console.log(`Server-reported version after update: ${firstUpdate.version} (bumped by the mock)`);
  assert(
    firstUpdate.version === 1,
    "a correct If-Match must bump the stored version — got " + firstUpdate.version,
  );

  // Optimistic locking, end to end. The mock enforces `If-Match` exactly
  // as a real cratestack-pg server does (#605), so the version we just
  // consumed is now stale and replaying it must be REJECTED.
  const staleResponse = await fetch(`${BASE_URL}/api/posts/${id}`, {
    method: "PATCH",
    headers: { "content-type": "application/json", "If-Match": '"0"' },
    body: JSON.stringify({ title: "Launch (replayed stale write)" }),
  });
  console.log(`\nReplayed stale If-Match PATCH -> HTTP ${staleResponse.status} (expected 412)`);
  assert(
    staleResponse.status === 412,
    `a stale If-Match must be rejected with 412, got ${staleResponse.status}`,
  );

  // The other three rows of the contract, so a regression in any one of
  // them fails this script rather than being noticed by a user.
  for (const [label, header, expected] of [
    ["absent If-Match", undefined, 412],
    ["If-Match: *", "*", 400],
    ["malformed If-Match", "0", 400],
  ] as const) {
    const response = await fetch(`${BASE_URL}/api/posts/${id}`, {
      method: "PATCH",
      headers: {
        "content-type": "application/json",
        ...(header ? { "If-Match": header } : {}),
      },
      body: JSON.stringify({ title: "should not apply" }),
    });
    console.log(`${label} -> HTTP ${response.status} (expected ${expected})`);
    assert(response.status === expected, `${label}: expected ${expected}, got ${response.status}`);
  }

  // Delete enforces If-Match too — the real server closed that asymmetry
  // deliberately, and the mock mirrors it. Version is 1 after the update.
  await client.posts.delete(id, { headers: { "If-Match": '"1"' } });
  const afterDelete = await client.posts.get(id).catch((error) => error);
  assert(
    afterDelete &&
      typeof afterDelete === "object" &&
      "status" in afterDelete &&
      afterDelete.status === 404,
    "get on a deleted post must 404",
  );
  console.log(
    "Post: falsy round trip verified; If-Match gap confirmed and documented; delete -> 404 verified.",
  );
}

async function verifyTag() {
  console.log("\n=== Tag (@id named `slug`) ===");
  const created = await client.tags.create({ slug: "topic-databases", label: "Databases" });
  const slug = created.slug!; // server-generated — see verifyCategory()'s comment
  assert(created.label === "Databases", "create should echo the submitted label");

  const updated = await client.tags.update(slug, { label: "Databases (updated)" });
  assert(updated.label === "Databases (updated)", "update should persist the new label");

  await client.tags.delete(slug);
  const afterDelete = await client.tags.get(slug).catch((error) => error);
  assert(
    afterDelete &&
      typeof afterDelete === "object" &&
      "status" in afterDelete &&
      afterDelete.status === 404,
    "get on a deleted tag must 404",
  );
  console.log("Tag: create -> update -> delete -> 404 verified with a non-`id` primary key.");
}

async function main() {
  await verifyCategory();
  await verifyPostFalsyRoundTripAndIfMatchGap();
  await verifyTag();
  console.log(`\n✓ All assertions passed (${requestCount} HTTP requests logged above).`);
}

main().catch((error) => {
  console.error("\n✗ verify FAILED:", error);
  process.exitCode = 1;
});
