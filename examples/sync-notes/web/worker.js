// One Dedicated Worker per device on the page. It owns the device's wasm
// instance (SQLite on OPFS + the cratestack-sync engine) and ALL of its
// networking: the engine is sans-IO, so this file is the transport.
//
// "Offline" is simulated per device: while it is set, this worker makes no
// requests at all and drops its poke stream — exactly what a phone in a
// tunnel looks like to the engine. Writes keep working locally.

import init, * as engine from './pkg/sync_notes_client.js';

let user = '';
let label = '';
let storage = '';
let online = true;
let syncing = false;
let again = false;
let pokeAbort = null;
let log = [];

const note = (line) => {
  log = [{ at: Date.now(), line }, ...log].slice(0, 6);
};

function emit() {
  postMessage({
    type: 'state',
    storage,
    online,
    syncing,
    log,
    notes: JSON.parse(engine.notes()),
    status: JSON.parse(engine.status()),
  });
}

class SyncError extends Error {
  constructor(body) {
    super(body.message);
    this.body = body;
  }
}

async function post(path, body) {
  if (!online) throw new Error('offline');
  const response = await fetch(path, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'x-demo-user': user },
    body,
  });
  const text = await response.text();
  if (!response.ok) throw new SyncError(JSON.parse(text));
  return text;
}

async function syncOnce() {
  if (!online) return;
  if (syncing) {
    again = true;
    return;
  }
  syncing = true;
  emit();
  try {
    if (!JSON.parse(engine.status()).clientId) {
      engine.applyRegister(await post('/sync/register', engine.registerRequest(label)));
      note('registered with the server');
      if (!pokeAbort) openPokes();
    }
    for (let req = engine.pushRequest(50); req; req = engine.pushRequest(50)) {
      const out = JSON.parse(engine.applyPush(await post('/sync/push', req)));
      note(`pushed ${out.applied + out.rejected}` + (out.rejected ? ` · ${out.rejected} rejected` : ''));
    }
    for (let req = engine.pullRequest(); req; ) {
      const out = JSON.parse(engine.applyPull(await post('/sync/pull', req)));
      if (out.changes) note(`pulled ${out.changes} change${out.changes === 1 ? '' : 's'}`);
      req = out.hasMore ? engine.pullRequest() : null;
    }
  } catch (error) {
    if (error instanceof SyncError) {
      const recovery = JSON.parse(engine.recover(JSON.stringify(error.body)));
      note(`${error.body.code} → ${recovery.action}`);
      if (recovery.action !== 'halt') again = true;
    } else if (online) {
      note(`network error: ${error.message}`);
    }
  } finally {
    syncing = false;
    emit();
    if (again && online) {
      again = false;
      setTimeout(syncOnce, 250);
    }
  }
}

// Server-sent pokes, read with fetch so the auth header can ride along.
async function openPokes() {
  pokeAbort?.abort();
  const status = JSON.parse(engine.status());
  if (!online || !status.clientId) return;
  const abort = new AbortController();
  pokeAbort = abort;
  try {
    const response = await fetch(`/sync/poke?clientId=${status.clientId}`, {
      headers: { 'x-demo-user': user },
      signal: abort.signal,
    });
    if (!response.ok) throw new Error(`poke ${response.status}`);
    const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    for (let buffer = ''; ; ) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += value;
      const events = buffer.split('\n\n');
      buffer = events.pop();
      if (events.some((e) => e.includes('event: poke'))) syncOnce();
    }
  } catch (_) {
    // Aborted (went offline) or dropped; reconnect below if still wanted.
  }
  if (online && pokeAbort === abort) setTimeout(openPokes, 1000);
}

function setOnline(value) {
  online = value;
  if (online) {
    note('back online');
    syncOnce().then(() => pokeAbort || openPokes());
  } else {
    note('offline — writes queue locally');
    pokeAbort?.abort();
    pokeAbort = null;
  }
  emit();
}

function write(action) {
  try {
    action();
  } catch (error) {
    note(`local write refused: ${error}`);
  }
  emit();
  syncOnce();
}

onmessage = async ({ data }) => {
  switch (data.type) {
    case 'init':
      await init();
      user = data.user;
      label = data.label;
      storage = await engine.open(user, data.directory);
      note(`opened ${storage === 'opfs' ? 'OPFS' : 'in-memory'} database`);
      emit();
      await syncOnce();
      if (!pokeAbort) openPokes();
      setInterval(syncOnce, 30_000);
      break;
    case 'create':
      write(() => engine.create(data.title));
      break;
    case 'update':
      write(() => engine.update(data.id, data.title ?? undefined, data.done ?? undefined));
      break;
    case 'remove':
      write(() => engine.remove(data.id));
      break;
    case 'online':
      setOnline(data.value);
      break;
    case 'clearRejected':
      engine.clearRejected();
      emit();
      break;
  }
};
