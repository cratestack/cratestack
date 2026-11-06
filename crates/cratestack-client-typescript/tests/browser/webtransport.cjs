// Real-browser check of the generated TypeScript client's WebTransport
// binding (slice S6): headless Chromium (Playwright) runs the generated
// `subscriptions.js` / `subscription-sockets.js`, compiled from a package
// generated off `tests/fixtures/subscriptions.cstack`, against
// `cratestack-webtransport`'s `browser_harness`.
//
//   node tests/browser/webtransport.cjs <browser_harness> <dir of the compiled modules>
//
// Run it through `just test-ts-webtransport-browser`. Exits non-zero, with
// the failing check named, on any mismatch.
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const readline = require("node:readline");
const { chromium } = require("playwright");

const [harnessPath, modulesDir] = process.argv.slice(2);
if (!harnessPath || !modulesDir) {
  console.error("usage: node tests/browser/webtransport.cjs <browser_harness> <modules dir>");
  process.exit(2);
}

function startHarness(allowedOrigin) {
  const child = spawn(harnessPath, [allowedOrigin], { stdio: ["ignore", "pipe", "inherit"] });
  const ready = new Promise((resolve, reject) => {
    child.on("exit", (code) => reject(new Error(`harness exited early (${code})`)));
    readline.createInterface({ input: child.stdout }).on("line", (line) => {
      const event = JSON.parse(line);
      if (event.port) resolve(event);
    });
  });
  return { child, ready };
}

// Runs inside the page: the generated client against the harness.
async function scenarios({ port, sha256 }) {
  const { subscribe } = await import("/subscriptions.js");
  const host = (token) => ({
    origin: location.origin,
    basePath: "/",
    fetchFn: fetch.bind(globalThis),
    defaultHeaders: { authorization: `Bearer ${token}` },
    subscriptions: {
      transport: "webtransport",
      webTransport: {
        url: `https://127.0.0.1:${port}/rpc/wt`,
        serverCertificateHashes: [{ algorithm: "sha-256", value: new Uint8Array(sha256) }],
      },
    },
  });
  const target = (opId, input) => ({ opId, input, ssePath: "/unused", sseQuery: "", socketPath: "/unused" });
  const identity = (value) => value;
  const out = { items: [] };
  let ready = false;
  for await (const item of subscribe(host("7"), target("test.items", { n: 3 }), { onReady: () => (ready = true) }, identity)) {
    out.items.push(item);
  }
  out.ready = ready;
  for await (const item of subscribe(host("7"), target("test.whoami", null), {}, identity)) {
    out.whoami = item;
    break;
  }
  try {
    for await (const _ of subscribe(host("7"), target("test.refuse", null), {}, identity)) {
      // A refusal never yields.
    }
  } catch (error) {
    out.refused = error.code;
  }
  const forever = subscribe(host("7"), target("test.forever", null), {}, identity)[Symbol.asyncIterator]();
  out.foreverDelivers = (await forever.next()).done === false;
  await forever.return();
  try {
    for await (const _ of subscribe(host("bad"), target("test.items", { n: 1 }), {}, identity)) {
      // A refused session never yields.
    }
  } catch (error) {
    out.badToken = error.code;
  }
  return out;
}

function check(name, condition, detail) {
  if (!condition) {
    console.error(`FAIL ${name}: ${JSON.stringify(detail)}`);
    process.exitCode = 1;
  } else {
    console.log(`ok   ${name}`);
  }
}

async function main() {
  const server = http.createServer((req, res) => {
    if (req.url.endsWith(".js")) {
      res.setHeader("content-type", "text/javascript");
      res.end(fs.readFileSync(path.join(modulesDir, path.basename(req.url))));
    } else {
      res.setHeader("content-type", "text/html");
      res.end("<!doctype html><title>generated client over webtransport</title>");
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://localhost:${server.address().port}`;
  const harness = startHarness(origin);
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.goto(`${origin}/`);
    const r = await page.evaluate(scenarios, await harness.ready);
    check("Ready, then three items, then the end", r.ready && JSON.stringify(r.items) === '[{"i":0},{"i":1},{"i":2}]', r);
    check("the client's headers reach the AuthProvider through Init", r.whoami === "7", r.whoami);
    check("a refused open is thrown", r.refused === "permission_denied", r.refused);
    check("a live subscription delivers and cancels", r.foreverDelivers, r);
    check("other credentials get their own session, and its refusal", r.badToken === "unauthenticated", r.badToken);
  } finally {
    harness.child.kill();
    await browser.close();
    server.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
