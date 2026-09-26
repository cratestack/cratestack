// The client side of the conformance run (cratestack#1041): one Inspector
// CLI invocation per case, and the bookkeeping that makes the run fail
// unless every expected case ran and passed. `run.mjs` holds the cases.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

export const env = (name) => {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set; run this through \`just mcp-conformance\``);
  return value;
};

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

/**
 * The installed Inspector, checked against the version `package.json` pins.
 * `pnpm install --frozen-lockfile` already refuses a lockfile that disagrees
 * with `package.json`; this catches a run pointed at some other install.
 */
export function installedInspector(inspectorDir) {
  const pinned = readJson(new URL("./package.json", import.meta.url)).dependencies[
    "@modelcontextprotocol/inspector"
  ];
  const inspectorRoot = realpathSync(join(inspectorDir, "node_modules/@modelcontextprotocol/inspector"));
  // pnpm links only direct dependencies into the top-level `node_modules`,
  // so the Inspector's own dependencies are found the way Node finds them
  // when it runs the Inspector: from the Inspector's real path.
  const require = createRequire(join(inspectorRoot, "package.json"));
  const pkg = (name) => {
    const found = (require.resolve.paths(name) ?? [])
      .map((dir) => join(dir, name, "package.json"))
      .find(existsSync);
    if (!found) throw new Error(`${name} is not installed beside the Inspector`);
    return readJson(found);
  };
  const inspector = readJson(join(inspectorRoot, "package.json"));
  if (inspector.version !== pinned) {
    throw new Error(`package.json pins inspector ${pinned}, installed ${inspector.version}`);
  }
  return {
    inspector,
    sdkClient: pkg("@modelcontextprotocol/client"),
    launcher: join(inspectorRoot, inspector.bin["mcp-inspector"]),
  };
}

/** One Inspector CLI run: `{ status, result, error, raw }`. */
export function inspect(launcher, home, target, options) {
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
        HOME: home,
        MCP_STORAGE_DIR: home,
        MCP_INSPECTOR_SECRET_STORE: "memory",
        MCP_AUTO_OPEN_ENABLED: "false",
      },
    },
  );
  // `--format json` prints the result as one JSON line on stdout, and an
  // `error` object as one JSON line on stderr when the call failed (a tool
  // `isError` prints both). A stdio server's own stderr passes through.
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

/** A token from the example's own `mint-token`; never an empty string. */
export function mint(exampleBin, signingKey, audience, id, role) {
  const run = spawnSync(exampleBin, ["mint-token", "--audience", audience, "--id", id, "--role", role], {
    encoding: "utf8",
    env: { ...process.env, MCP_EXAMPLE_SIGNING_KEY: signingKey },
  });
  const token = run.stdout?.trim();
  // An empty token would make every refusal case pass for the wrong reason.
  if (run.status !== 0 || !token) {
    throw new Error(`mint-token failed (exit ${run.status}): ${run.stderr}`);
  }
  return token;
}

export const results = [];

export function check(transport, name, run, predicate) {
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

/** Exits non-zero unless exactly `expected` distinct cases ran and all passed. */
export function finish(expected) {
  const failed = results.filter((result) => !result.ok);
  const distinct = new Set(results.map((result) => `${result.transport} ${result.name}`)).size;
  console.log(`\n${results.length - failed.length}/${results.length} passed (expected ${expected} cases)`);
  if (distinct !== results.length) console.log("FAIL: a case ran twice under one name");
  if (failed.length > 0 || results.length !== expected || distinct !== results.length) process.exit(1);
}

export const text = (run) => JSON.parse(run.result.content[0].text);
export const doc = (run) => JSON.parse(run.result.contents[0].text);
export const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
export const ids = (rows) => rows.map((row) => row.id).sort((a, b) => a - b);
