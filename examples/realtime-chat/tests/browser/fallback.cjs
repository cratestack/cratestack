// The chat in headless Chromium (Playwright), on the generated TypeScript
// client's `"auto"` (slice S6, docs/design/subscriptions-websocket.md §8):
//
// - UDP open: the typing subscription runs over WebTransport, pinned to the
//   server's dev certificate, and no WebSocket is opened;
// - UDP blocked (the WebTransport URL names a UDP port nothing answers on):
//   the same subscription falls back to the WebSocket;
// - SSE, the binding of last resort, chosen explicitly.
//
// In each, a unary `sendTyping` call publishes and the subscription sees it.
//
//   node tests/browser/fallback.cjs <realtime-chat-server> <static dir>
//
// `<static dir>` holds the compiled `subscriptions.js` and
// `subscription-sockets.js` (from a package generated off `schema.cstack`)
// and `index.html`; the server serves it at `/`, so the page shares the
// HTTP listener's origin. Needs `DATABASE_URL`. Run it through
// `just test-realtime-chat`.
const { spawn } = require("node:child_process");
const dgram = require("node:dgram");
const readline = require("node:readline");
const { chromium } = require("playwright");

const [serverPath, staticDir] = process.argv.slice(2);
if (!serverPath || !staticDir || !process.env.DATABASE_URL) {
  console.error("usage: DATABASE_URL=... node tests/browser/fallback.cjs <server> <static dir>");
  process.exit(2);
}

function startServer() {
  const child = spawn(serverPath, [], {
    env: {
      ...process.env,
      REALTIME_CHAT_HTTP: "127.0.0.1:0",
      REALTIME_CHAT_WT: "127.0.0.1:0",
      REALTIME_CHAT_STATIC: staticDir,
    },
    stdio: ["ignore", "pipe", "inherit"],
  });
  const ready = new Promise((resolve, reject) => {
    child.on("exit", (code) => reject(new Error(`server exited early (${code})`)));
    readline.createInterface({ input: child.stdout }).on("line", (line) => {
      const started = JSON.parse(line);
      if (started.http) resolve(started);
    });
  });
  return { child, ready };
}

// A UDP port nothing listens on: bound, read, released.
function deadUdpPort() {
  return new Promise((resolve) => {
    const socket = dgram.createSocket("udp4");
    socket.bind(0, "127.0.0.1", () => {
      const { port } = socket.address();
      socket.close(() => resolve(port));
    });
  });
}

// Runs inside the page.
async function typingOnce({ transport, webTransport, room, who }) {
  const { subscribe, procedureQuery } = await import("/subscriptions.js");
  const headers = { authorization: "Bearer 1" };
  const host = {
    origin: location.origin,
    basePath: "/",
    fetchFn: fetch.bind(globalThis),
    defaultHeaders: headers,
    subscriptions: {
      transport,
      webTransport: webTransport && {
        url: webTransport.url,
        serverCertificateHashes: [{ algorithm: "sha-256", value: new Uint8Array(webTransport.sha256) }],
      },
    },
  };
  const args = { args: { roomId: room } };
  const target = {
    opId: "procedure.typing",
    input: args,
    ssePath: "/rpc/subscribe/procedure.typing",
    sseQuery: procedureQuery(args),
    socketPath: "/rpc/ws",
  };
  let ready;
  const live = new Promise((resolve) => (ready = resolve));
  const iterator = subscribe(host, target, { onReady: ready }, (value) => value)[Symbol.asyncIterator]();
  const first = iterator.next();
  await live;
  const sent = await fetch("/rpc/procedure.sendTyping", {
    method: "POST",
    headers: { ...headers, "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify({ args: { roomId: room, who } }),
  });
  if (!sent.ok) throw new Error(`sendTyping: ${sent.status} ${await sent.text()}`);
  const item = await first;
  await iterator.return();
  return item.value;
}

function check(name, condition, detail) {
  if (!condition) {
    console.error(`FAIL ${name}: ${JSON.stringify(detail)}`);
    process.exitCode = 1;
  } else {
    console.log(`ok   ${name}`);
  }
}

async function scenario(browser, origin, input) {
  // A fresh context per scenario: `"auto"` remembers a failed WebTransport
  // origin in localStorage.
  const context = await browser.newContext();
  const page = await context.newPage();
  const sockets = [];
  const sse = [];
  page.on("websocket", (socket) => sockets.push(socket.url()));
  page.on("request", (request) => {
    if (request.url().includes("/rpc/subscribe/")) sse.push(request.url());
  });
  await page.goto(`${origin}/index.html`);
  const item = await page.evaluate(typingOnce, input);
  await context.close();
  return { item, sockets, sse };
}

async function main() {
  const server = startServer();
  const browser = await chromium.launch();
  try {
    const { http, webtransport, sha256 } = await server.ready;
    const open = await scenario(browser, http, {
      transport: "auto",
      webTransport: { url: webtransport, sha256 },
      room: "open",
      who: "ada",
    });
    check("UDP open: auto delivers over WebTransport", open.item?.who === "ada", open);
    check(
      "UDP open: neither a WebSocket nor SSE is opened",
      open.sockets.length === 0 && open.sse.length === 0,
      open,
    );

    const blocked = await scenario(browser, http, {
      transport: "auto",
      webTransport: { url: `https://127.0.0.1:${await deadUdpPort()}/rpc/wt`, sha256 },
      room: "blocked",
      who: "grace",
    });
    check("UDP blocked: auto still delivers", blocked.item?.who === "grace", blocked);
    check(
      "UDP blocked: it fell back to the WebSocket",
      blocked.sockets.length === 1 && blocked.sockets[0].endsWith("/rpc/ws") && blocked.sse.length === 0,
      blocked,
    );

    const sse = await scenario(browser, http, { transport: "sse", room: "sse", who: "linus" });
    check("SSE delivers", sse.item?.who === "linus", sse);
    check("SSE opens no WebSocket", sse.sockets.length === 0 && sse.sse.length === 1, sse);
  } finally {
    server.child.kill();
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
