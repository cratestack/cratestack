// Main thread: render each device from its worker's state messages, and
// poll the demo-only /demo/server endpoint for the server panel.

const escape = (s) =>
  String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const short = (id) => String(id).slice(0, 8);
const ago = (ms) => {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  return s < 2 ? 'now' : s < 60 ? `${s}s` : `${Math.round(s / 60)}m`;
};

const template = document.getElementById('device-template');

for (const card of document.querySelectorAll('.device')) {
  const { device, user, label } = card.dataset;
  card.append(template.content.cloneNode(true));
  card.querySelector('.label').textContent = label;
  const avatar = card.querySelector('.avatar');
  avatar.textContent = user[0].toUpperCase();
  avatar.classList.add(user);

  const worker = new Worker('worker.js', { type: 'module' });
  const send = (message) => worker.postMessage(message);
  send({ type: 'init', user, label, directory: `.sync-notes-${device}` });
  worker.onmessage = ({ data }) => render(card, data, send);

  card.querySelector('.add').addEventListener('submit', (event) => {
    event.preventDefault();
    const input = card.querySelector('.title');
    if (input.value.trim()) send({ type: 'create', title: input.value.trim() });
    input.value = '';
  });
  card.querySelector('.online').addEventListener('change', (event) => {
    send({ type: 'online', value: event.target.checked });
  });
  card.querySelector('.dismiss').addEventListener('click', () => send({ type: 'clearRejected' }));
  card.querySelector('.notes').addEventListener('click', (event) => {
    const item = event.target.closest('li[data-id]');
    if (!item) return;
    const id = item.dataset.id;
    if (event.target.matches('.done')) send({ type: 'update', id, done: event.target.checked });
    if (event.target.matches('.delete')) send({ type: 'remove', id });
    if (event.target.matches('.edit')) {
      const title = prompt('Rename note', item.querySelector('.text').textContent);
      if (title && title.trim()) send({ type: 'update', id, title: title.trim() });
    }
  });
}

function render(card, state, send) {
  const { status, notes, online, syncing } = state;
  card.querySelector('.storage').textContent =
    state.storage === 'opfs' ? 'SQLite on OPFS · wasm worker' : 'SQLite in memory · wasm worker';
  card.querySelector('.online').checked = online;
  card.querySelector('.switch-label').textContent = online ? 'Online' : 'Offline';
  card.classList.toggle('is-offline', !online);

  const [cls, text] = !online
    ? ['off', status.unpushed ? `Offline — ${status.unpushed} change${status.unpushed > 1 ? 's' : ''} queued locally` : 'Offline — local writes still work']
    : syncing
      ? ['busy', 'Syncing…']
      : status.pending
        ? ['wait', `${status.pending} change${status.pending > 1 ? 's' : ''} awaiting server ack`]
        : ['ok', 'In sync with the server'];
  card.querySelector('.state .dot').className = `dot ${cls}`;
  card.querySelector('.state-text').textContent = text;

  card.querySelector('.notes').innerHTML = notes.length
    ? notes
        .map(
          (n) => `<li data-id="${n.id}" class="${n.done ? 'is-done' : ''}">
            <input type="checkbox" class="done" ${n.done ? 'checked' : ''} aria-label="done">
            <span class="text">${escape(n.title)}</span>
            ${n.unsynced ? '<span class="badge" title="local change not yet acknowledged by the server">unsynced</span>' : ''}
            <button class="icon edit" title="Rename" aria-label="Rename">✎</button>
            <button class="icon delete" title="Delete" aria-label="Delete">✕</button>
          </li>`,
        )
        .join('')
    : '<li class="empty">No notes on this device.</li>';

  const rejected = card.querySelector('.rejected');
  rejected.hidden = !status.rejected.length;
  rejected.querySelector('ul').innerHTML = status.rejected
    .map((r) => `<li><code>${escape(r.name)}</code> <span class="code">${escape(r.code)}</span> ${escape(r.message)}</li>`)
    .join('');

  card.querySelector('.stats').innerHTML = `
    <div><dt>client</dt><dd>${status.clientId ? short(status.clientId) : '—'}</dd></div>
    <div><dt>cursor</dt><dd>${escape(status.cursor)}</dd></div>
    <div><dt>queued</dt><dd>${status.unpushed}</dd></div>
    <div><dt>awaiting ack</dt><dd>${status.pending - status.unpushed}</dd></div>`;
  card.querySelector('.log').innerHTML = state.log
    .map((l) => `<li><span>${ago(l.at)}</span>${escape(l.line)}</li>`)
    .join('');
}

// ---- server panel ---------------------------------------------------------

let serverNotes = [];

async function refreshServer() {
  try {
    const state = await (await fetch('/demo/server')).json();
    serverNotes = state.notes;
    document.querySelector('#server-notes tbody').innerHTML =
      state.notes
        .map((n) => `<tr><td><span class="owner ${escape(n.ownerId)}">${escape(n.ownerId)}</span></td><td>${escape(n.title)}</td><td>${n.done ? '✓' : ''}</td></tr>`)
        .join('') || '<tr><td colspan="3" class="empty">empty</td></tr>';
    document.querySelector('#server-changes tbody').innerHTML =
      state.changes
        .map((c) => `<tr><td><code>${escape(c.position)}</code></td><td>${escape(c.scope.startsWith('client:') ? 'client:' + short(c.scope.slice(7)) : c.scope)}</td><td class="op ${c.op}">${c.model === '$ack' ? 'ack' : c.op}</td><td><code>${short(c.pk)}</code></td></tr>`)
        .join('') || '<tr><td colspan="4" class="empty">empty</td></tr>';
    document.querySelector('#server-clients tbody').innerHTML = state.clients
      .map((c) => `<tr><td>${escape(c.principal)}</td><td><code>${short(c.clientId)}</code></td><td>${c.lastMutationSeq}</td></tr>`)
      .join('');
    const select = document.getElementById('server-note');
    const chosen = select.value;
    select.innerHTML = state.notes
      .map((n) => `<option value="${n.id}">${escape(n.ownerId)}: ${escape(n.title)}</option>`)
      .join('');
    if (chosen) select.value = chosen;
  } catch (_) {}
}

document.getElementById('server-edit').addEventListener('submit', async (event) => {
  event.preventDefault();
  const id = document.getElementById('server-note').value;
  const title = document.getElementById('server-title').value.trim();
  const target = serverNotes.find((n) => n.id === id);
  if (!target || !title) return;
  const response = await fetch(`/api/notes/${id}`, {
    method: 'PATCH',
    headers: { 'content-type': 'application/json', 'x-demo-user': target.ownerId },
    body: JSON.stringify({ title }),
  });
  document.getElementById('server-result').textContent =
    `PATCH /api/notes/${short(id)} as ${target.ownerId} → ${response.status}. The trigger logged it; the owner's devices get a poke.`;
  document.getElementById('server-title').value = '';
  refreshServer();
});

refreshServer();
setInterval(refreshServer, 1000);
