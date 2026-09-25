// Conformance run for the MCP operator example (cratestack#1041): the
// official MCP Inspector CLI (`@modelcontextprotocol/inspector`, a
// third-party client built on the official TypeScript SDK, not on `rmcp`)
// drives `mcp-operator-example` over stdio and over Streamable HTTP.
//
// Run it through `just mcp-conformance`, which installs the client from
// `package-lock.json`, builds the example and provides Postgres. This file
// only drives the client and checks what it reports.
//
// Every case asserts on content, not only on an exit code, and the run fails
// unless every expected case ran: a client that cannot connect, or a table
// that loses a row, is a failure, never a skip.

import { spawn, spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";

const env = (name) => {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set; run this through \`just mcp-conformance\``);
  return value;
};

const INSPECTOR_DIR = env("MCP_INSPECTOR_DIR");
const EXAMPLE_BIN = env("MCP_EXAMPLE_BIN");
const DATABASE_URL = env("DATABASE_URL");
// `npm ci` already refuses a lockfile that disagrees with `package.json`;
// this catches a run pointed at some other install.
const EXPECTED_INSPECTOR = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"))
  .dependencies["@modelcontextprotocol/inspector"];
const SIGNING_KEY = "conformance-only-signing-key, not a secret, 0123456789";
const PROTOCOL = "2026-07-28";
const STDIO_AUDIENCE = "cratestack://blog";

const pkg = (name) =>
  JSON.parse(readFileSync(join(INSPECTOR_DIR, "node_modules", name, "package.json"), "utf8"));
const inspector = pkg("@modelcontextprotocol/inspector");
const sdkClient = pkg("@modelcontextprotocol/client");
const launcher = join(INSPECTOR_DIR, "node_modules/@modelcontextprotocol/inspector", inspector.bin["mcp-inspector"]);

// ---- the client ---------------------------------------------------------

/** One Inspector CLI run: `{ status, result, error, raw }`. */
function inspect(target, options) {
  const run = spawnSync(
    process.execPath,
    [launcher, "--cli", ...target, "--", "--format", "json", "--connect-timeout", "30000", ...options],
    {
      encoding: "utf8",
      timeout: 120_000,
      env: {
        ...process.env,
        // An isolated home and an in-memory secret store: no stored OAuth
        // token, catalog entry or OS keychain item from the machine running
        // this can change what the client does, and it writes none.
        HOME: env("MCP_INSPECTOR_HOME"),
        MCP_STORAGE_DIR: env("MCP_INSPECTOR_HOME"),
        MCP_INSPECTOR_SECRET_STORE: "memory",
        MCP_AUTO_OPEN_ENABLED: "false",
      },
    },
  );
  // `--format json` prints the result as one JSON line on stdout, and an
  // `error` object as one JSON line on stderr when the call failed (a tool
  // `isError` prints both).
  const lines = `${run.stdout ?? ""}\n${run.stderr ?? ""}`
    .split("\n")
    .filter((line) => line.startsWith("{"));
  const objects = lines.map((line) => JSON.parse(line));
  return {
    status: run.status,
    result: objects.find((o) => "result" in o)?.result,
    error: objects.find((o) => "error" in o)?.error,
    raw: `${run.stdout ?? ""}${run.stderr ?? ""}`.trim(),
  };
}

const mint = (audience, id, role) =>
  spawnSync(EXAMPLE_BIN, ["mint-token", "--audience", audience, "--id", id, "--role", role], {
    encoding: "utf8",
    env: { ...process.env, MCP_EXAMPLE_SIGNING_KEY: SIGNING_KEY },
  }).stdout.trim();

function stdioTarget(token, era = "modern") {
  return {
    name: "stdio",
    target: [EXAMPLE_BIN, "stdio"],
    options: [
      "--protocol-era", era,
      "-e", `DATABASE_URL=${DATABASE_URL}`,
      "-e", `MCP_EXAMPLE_SIGNING_KEY=${SIGNING_KEY}`,
      "-e", `MCP_EXAMPLE_TOKEN=${token}`,
    ],
  };
}

function httpTarget(url, token) {
  const auth = token ? ["--header", `Authorization: Bearer ${token}`] : [];
  // `--stored-auth-only`: a 401 must end the run, never open a browser.
  return {
    name: "http",
    target: [url],
    options: ["--transport", "http", "--protocol-era", "modern", "--stored-auth-only", ...auth],
  };
}

const call = (transport, ...options) =>
  inspect(transport.target, [...transport.options, ...options]);

// ---- assertions ---------------------------------------------------------

const results = [];

function check(transport, name, run, predicate) {
  let failure;
  try {
    failure = predicate(run) === true ? undefined : "unexpected answer";
  } catch (error) {
    failure = error.message;
  }
  const summary = JSON.stringify(run.result ?? run.error ?? null);
  results.push({ transport, name, ok: !failure });
  console.log(`${failure ? "FAIL" : "ok  "} [${transport}] ${name}`);
  console.log(`       exit ${run.status}: ${summary.length > 400 ? `${summary.slice(0, 400)}…` : summary}`);
  if (failure) console.log(`       why: ${failure}\n       output: ${run.raw}`);
}

const text = (run) => JSON.parse(run.result.content[0].text);
const doc = (run) => JSON.parse(run.result.contents[0].text);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const ids = (rows) => rows.map((row) => row.id).sort((a, b) => a - b);

/** The cases every transport must pass, as caller `u-1`. */
function suite(member, editor, author) {
  const t = member.name;

  check(t, "discover: server/discover negotiates 2026-07-28", call(member, "--method", "initialize"),
    (r) => r.status === 0 && r.result.protocolVersion === PROTOCOL);

  check(t, "tools/list: exactly the two annotated procedures", call(member, "--method", "tools/list"),
    (r) => {
      const tools = r.result.tools;
      return same(tools.map((tool) => tool.name), ["recent_posts", "publish_post"])
        && tools[0].annotations.readOnlyHint === true
        && tools[1].annotations.readOnlyHint === false
        && tools[1].annotations.idempotentHint === false;
    });

  check(t, "tools/call recent_posts: succeeds, rows filtered by @@allow in SQL",
    call(member, "--method", "tools/call", "--tool-name", "recent_posts", "--tool-args-json", '{"limit":10}'),
    (r) => r.status === 0 && r.result.isError === false && same(ids(text(r)), [1, 2, 4]));

  // The envelope is the server's own REST error body. The Inspector also
  // synthesises an `isError` result when a call fails client-side, with
  // plain text, so parsing `FORBIDDEN` out of it proves the server said so.
  // The message pins *which* policy refused: the procedure's `@allow`,
  // before the implementation ran. With that `@allow` loosened, the
  // model's `@@allow("update", ...)` still refuses in SQL, but as "update
  // policy denied", and this case fails.
  check(t, "tools/call publish_post as a member: policy-denied, isError + FORBIDDEN",
    call(member, "--method", "tools/call", "--tool-name", "publish_post", "--tool-args-json", '{"id":3}'),
    (r) => r.result.isError === true && r.error.code === "tool_is_error"
      && same(text(r), { code: "FORBIDDEN", message: "procedure policy denied this operation", details: null }));

  check(t, "the denied call never ran: post 3 is still a draft (read as its author)",
    call(author, "--method", "resources/read", "--uri", "cratestack://blog/posts/3"),
    (r) => r.status === 0 && doc(r).published === false);

  check(t, "tools/call publish_post as an editor: succeeds, structuredContent",
    call(editor, "--method", "tools/call", "--tool-name", "publish_post", "--tool-args-json", '{"id":2}'),
    (r) => r.status === 0 && r.result.isError === false
      && r.result.structuredContent.id === 2 && r.result.structuredContent.published === true);

  check(t, "resources/list: one collection, named by segment not table",
    call(member, "--method", "resources/list"),
    (r) => same(r.result.resources.map((res) => res.uri), ["cratestack://blog/posts"]));

  check(t, "resources/read one record", call(member, "--method", "resources/read", "--uri", "cratestack://blog/posts/1"),
    (r) => r.status === 0 && doc(r).id === 1 && r.result.cacheScope === "private");

  check(t, "resources/read the collection: only readable rows",
    call(member, "--method", "resources/read", "--uri", "cratestack://blog/posts?limit=10"),
    (r) => r.status === 0 && same(ids(doc(r).items), [1, 2, 4]));

  const hidden = call(member, "--method", "resources/read", "--uri", "cratestack://blog/posts/3");
  const missing = call(member, "--method", "resources/read", "--uri", "cratestack://blog/posts/999");
  check(t, "resources/read a row @@allow hides: refused like a missing row", hidden,
    (r) => r.status !== 0 && r.result === undefined && same(r.error, missing.error)
      && /not found/.test(r.error.message));
}

// ---- the run ------------------------------------------------------------

const freePort = () =>
  new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });

async function startHttp(port) {
  const url = `http://127.0.0.1:${port}/mcp`;
  const child = spawn(EXAMPLE_BIN, ["http", "--addr", `127.0.0.1:${port}`], {
    env: { ...process.env, DATABASE_URL, MCP_EXAMPLE_SIGNING_KEY: SIGNING_KEY },
    stdio: ["ignore", "inherit", "inherit"],
  });
  const metadata = `http://127.0.0.1:${port}/.well-known/oauth-protected-resource`;
  for (let attempt = 0; attempt < 150; attempt += 1) {
    if (child.exitCode !== null) throw new Error(`the HTTP server exited with ${child.exitCode}`);
    try {
      const response = await fetch(metadata);
      if (response.ok) return { child, url, metadata: await response.json() };
    } catch {
      // not listening yet
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  child.kill();
  throw new Error("the HTTP server did not start within 30s");
}

console.log(`client: ${inspector.name} ${inspector.version} (${inspector.repository.url})`);
console.log(`        on ${sdkClient.name} ${sdkClient.version}`);
console.log(`server: mcp-operator-example, cratestack-mcp, protocol ${PROTOCOL}\n`);
if (inspector.version !== EXPECTED_INSPECTOR) {
  throw new Error(`expected inspector ${EXPECTED_INSPECTOR}, installed ${inspector.version}`);
}

const http = await startHttp(await freePort());
try {
  // stdio: the identity is the token in the server's environment.
  suite(
    stdioTarget(mint(STDIO_AUDIENCE, "u-1", "member")),
    stdioTarget(mint(STDIO_AUDIENCE, "u-1", "editor")),
    stdioTarget(mint(STDIO_AUDIENCE, "u-2", "member")),
  );
  check("stdio", "a token for the HTTP audience does not start a stdio server",
    call(stdioTarget(mint(http.url, "u-1", "member")), "--method", "tools/list"),
    (r) => r.status !== 0 && r.result === undefined);
  check("stdio", "control: a legacy-era (initialize) client is refused, not negotiated down",
    call(stdioTarget(mint(STDIO_AUDIENCE, "u-1", "member"), "legacy"), "--method", "tools/list"),
    (r) => r.status !== 0 && /Unsupported protocol version/.test(r.raw));

  // Streamable HTTP: a bearer token whose audience is this endpoint's URL.
  suite(
    httpTarget(http.url, mint(http.url, "u-1", "member")),
    httpTarget(http.url, mint(http.url, "u-1", "editor")),
    httpTarget(http.url, mint(http.url, "u-2", "member")),
  );
  check("http", "RFC 9728 metadata names this resource and its issuer", { status: 0, result: http.metadata },
    (r) => r.result.resource === http.url && same(r.result.authorization_servers, ["https://auth.example.test"]));
  check("http", "no token: 401, the client stops at auth_required",
    call(httpTarget(http.url), "--method", "tools/list"),
    (r) => r.status !== 0 && r.error.code === "auth_required");
  check("http", "a token for another audience (stdio's): 401, auth_required",
    call(httpTarget(http.url, mint(STDIO_AUDIENCE, "u-1", "member")), "--method", "tools/list"),
    (r) => r.status !== 0 && r.error.code === "auth_required");
} finally {
  http.child.kill();
}

const EXPECTED_CASES = 2 * 10 + 2 + 3;
const failed = results.filter((result) => !result.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed (expected ${EXPECTED_CASES} cases)`);
if (failed.length > 0 || results.length !== EXPECTED_CASES) process.exit(1);
