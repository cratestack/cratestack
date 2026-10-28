// Real-browser end-to-end check of the WebTransport binding: headless
// Chromium (Playwright) against `examples/browser_harness.rs`, speaking the
// binding's JSON codec through the browser's own `WebTransport` API.
//
//   node tests/browser/run.cjs <path-to-browser_harness>
//
// Run it through `just test-webtransport-browser`, which builds the harness
// first. Exits non-zero, with the failing scenario named, on any mismatch.
const { spawn } = require("node:child_process");
const http = require("node:http");
const readline = require("node:readline");
const { chromium } = require("playwright");

const harnessPath = process.argv[2];
if (!harnessPath) {
  console.error("usage: node tests/browser/run.cjs <path-to-browser_harness>");
  process.exit(2);
}

function startHarness(allowedOrigin) {
  const child = spawn(harnessPath, [allowedOrigin], { stdio: ["ignore", "pipe", "inherit"] });
  const events = [];
  const ready = new Promise((resolve, reject) => {
    child.on("exit", (code) => reject(new Error(`harness exited early (${code})`)));
    readline.createInterface({ input: child.stdout }).on("line", (line) => {
      const event = JSON.parse(line);
      if (event.port) resolve(event);
      else events.push(event);
    });
  });
  return { child, events, ready };
}

// Everything below `scenarios` runs inside the page.
async function scenarios({ port, sha256, expectRefusedOrigin }) {
  const json = "cratestack.rpc.v1+json";
  const enc = new TextEncoder();
  const out = {};
  const open = async () => {
    const wt = new WebTransport(`https://127.0.0.1:${port}/rpc/wt`, {
      serverCertificateHashes: [{ algorithm: "sha-256", value: new Uint8Array(sha256) }],
      protocols: [json],
    });
    await wt.ready;
    return wt;
  };
  // NDJSON frames off a WebTransport receive stream.
  const frames = (readable) => {
    const reader = readable.pipeThrough(new TextDecoderStream()).getReader();
    let buffer = "";
    return {
      reader,
      async next() {
        for (;;) {
          const newline = buffer.indexOf("\n");
          if (newline >= 0) {
            const line = buffer.slice(0, newline);
            buffer = buffer.slice(newline + 1);
            return JSON.parse(line);
          }
          const { value, done } = await reader.read();
          if (done) return null;
          buffer += value;
        }
      },
    };
  };
  const send = async (writable, frame, close) => {
    const writer = writable.getWriter();
    await writer.write(enc.encode(`${JSON.stringify(frame)}\n`));
    if (close) await writer.close();
    else writer.releaseLock();
  };
  const session = async (token) => {
    const wt = await open();
    const control = await wt.createBidirectionalStream();
    const headers = token ? { authorization: `Bearer ${token}` } : {};
    await send(control.writable, { id: 0, init: { headers } }, false);
    return { wt, control, controlFrames: frames(control.readable) };
  };
  const subscribe = async (wt, id, op, input) => {
    const stream = await wt.createBidirectionalStream();
    await send(stream.writable, { id, op, input }, true);
    return frames(stream.readable);
  };

  if (expectRefusedOrigin) {
    try {
      await open();
      out.originRefused = false;
    } catch (error) {
      out.originRefused = String(error.name);
    }
    return out;
  }

  const s = await session("7");
  out.protocol = s.wt.protocol;
  out.initReply = await s.controlFrames.next();

  const items = await subscribe(s.wt, 1, "test.items", { n: 3 });
  out.items = [];
  for (let frame = await items.next(); frame; frame = await items.next()) out.items.push(frame);

  const who = await subscribe(s.wt, 2, "test.whoami", null);
  await who.next();
  out.whoami = (await who.next()).next;

  const refused = await subscribe(s.wt, 3, "test.refuse", null);
  out.refused = await refused.next();

  const forever = await subscribe(s.wt, 4, "test.forever", null);
  out.foreverReady = await forever.next();
  out.foreverFirst = await forever.next();
  await forever.reader.cancel();
  await new Promise((resolve) => setTimeout(resolve, 300));

  const bad = await session("bad");
  const closed = await Promise.race([
    bad.wt.closed,
    new Promise((_, reject) => setTimeout(() => reject(new Error("no close in 3s")), 3000)),
  ]);
  out.badTokenClose = { closeCode: closed.closeCode, reason: closed.reason };
  s.wt.close();
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
  const page = http.createServer((_, res) => {
    res.setHeader("content-type", "text/html");
    res.end("<!doctype html><title>cratestack webtransport</title>");
  });
  await new Promise((resolve) => page.listen(0, "127.0.0.1", resolve));
  const origin = `http://localhost:${page.address().port}`;
  const browser = await chromium.launch();
  const tab = await browser.newPage();
  await tab.goto(`${origin}/`);
  console.log(`browser: ${browser.version()}`);

  const allowed = startHarness(origin);
  const refusing = startHarness("https://somewhere-else.example");
  try {
    const { port, sha256 } = await allowed.ready;
    const r = await tab.evaluate(scenarios, { port, sha256, expectRefusedOrigin: false });
    check("subprotocol negotiated", r.protocol === "cratestack.rpc.v1+json", r.protocol);
    check("Init answered with Ready{0}", r.initReply?.id === 0 && r.initReply.ready, r.initReply);
    const kinds = r.items.map((f) => Object.keys(f).find((k) => k !== "id"));
    check(
      "ready, 3 items, end, FIN",
      JSON.stringify(kinds) === '["ready","next","next","next","end"]',
      r.items,
    );
    check("Init credentials reach the AuthProvider", r.whoami === "7", r.whoami);
    check("a refused open is an error frame", r.refused?.err?.code === "permission_denied", r.refused);
    check("a live subscription delivers", r.foreverReady?.ready && "next" in r.foreverFirst, r);
    check(
      "a browser cancel drops the dispatcher stream",
      allowed.events.some((e) => e.dropped),
      allowed.events,
    );
    check("bad credentials close with 4401", r.badTokenClose.closeCode === 4401, r.badTokenClose);

    const other = await refusing.ready;
    const refused = await tab.evaluate(scenarios, { ...other, expectRefusedOrigin: true });
    check(
      "a cross-origin page cannot open a session",
      refused.originRefused === "WebTransportError",
      refused,
    );
  } finally {
    allowed.child.kill();
    refusing.child.kill();
    await browser.close();
    page.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
