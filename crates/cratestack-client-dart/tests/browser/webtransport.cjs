// Real-browser check of the generated Dart client's WebTransport binding
// (slice S6): headless Chromium (Playwright) runs `webtransport_entry.dart`,
// compiled to JavaScript against a package generated off
// `tests/fixtures/subscriptions_rpc.cstack`, against
// `cratestack-webtransport`'s `browser_harness`.
//
//   node tests/browser/webtransport.cjs <browser_harness> <dir with main.dart.js>
//
// Run it through `just test-dart-webtransport-browser`. Exits non-zero, with
// the failing check named, on any mismatch.
const { spawn } = require("node:child_process");
const dgram = require("node:dgram");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const readline = require("node:readline");
const { chromium } = require("playwright");

const [harnessPath, compiledDir] = process.argv.slice(2);
if (!harnessPath || !compiledDir) {
  console.error("usage: node tests/browser/webtransport.cjs <browser_harness> <compiled dir>");
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

function deadPort() {
  return new Promise((resolve) => {
    const socket = dgram.createSocket("udp4");
    socket.bind(0, "127.0.0.1", () => {
      const { port } = socket.address();
      socket.close(() => resolve(port));
    });
  });
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
      res.end(fs.readFileSync(path.join(compiledDir, path.basename(req.url))));
    } else {
      res.setHeader("content-type", "text/html");
      res.end("<!doctype html><title>dart client over webtransport</title>");
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://localhost:${server.address().port}`;
  const harness = startHarness(origin);
  const browser = await chromium.launch();
  try {
    const started = await harness.ready;
    const page = await browser.newPage();
    page.on("console", (message) => console.log(`  [page] ${message.text()}`));
    await page.goto(`${origin}/`);
    await page.evaluate(
      (value) => {
        window.__harness = value;
      },
      JSON.stringify({ port: started.port, sha256: started.sha256, deadPort: await deadPort() }),
    );
    await page.addScriptTag({ url: "/main.dart.js" });
    await page.waitForFunction(() => window.__result !== undefined, null, { timeout: 30000 });
    const r = JSON.parse(await page.evaluate(() => window.__result));
    check("no error", r.error === undefined, r.error);
    check("the platform has WebTransport", r.supported === true, r);
    check("Ready, then three items, then the end", r.ready && JSON.stringify(r.items) === '[{"i":0},{"i":1},{"i":2}]', r);
    check("the credentials reach the AuthProvider through Init", r.whoami === "7", r.whoami);
    check("a refused open is thrown", r.refused === "permission_denied", r.refused);
    check("a refused session is thrown", r.badToken === "unauthenticated", r.badToken);
    check("a live subscription cancels", r.cancelled === true, r);
    check("auto takes WebTransport, never SSE", JSON.stringify(r.auto) === '[{"i":0},{"i":1}]' && r.autoUsedHttp === false, r);
    check(
      "auto with UDP blocked moves on within its bound",
      r.fellBackToSse === true && r.fallbackMillis < 10000,
      { fellBackToSse: r.fellBackToSse, fallbackMillis: r.fallbackMillis },
    );
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
