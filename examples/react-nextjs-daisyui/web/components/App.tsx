'use client';

import { useState } from 'react';
import { useLocalNotes } from './useLocalNotes';
import { useSync } from './useSync';
import { LocalTab } from './LocalTab';
import { ServerTab } from './ServerTab';
import { RemoteTab } from './RemoteTab';

type Tab = 'local' | 'server' | 'remote';

export function App() {
  const local = useLocalNotes();
  const sync = useSync(local.kind === 'ready' ? local.client : null);
  const [tab, setTab] = useState<Tab>('local');

  // Server-tab cares about sync.lastSyncAt because a sync round-trip may
  // have written new rows on the napi side that the server view should
  // pick up. Drives the `revision` prop below.
  const revision = sync.lastSyncAt ?? '0';

  return (
    <div className="app-shell">
      <header className="navbar bg-base-100 shadow-sm">
        <div className="flex-1">
          <span className="btn btn-ghost text-xl normal-case">
            cratestack · Next.js + DaisyUI
          </span>
        </div>
        <div className="flex-none">
          <SyncBadge sync={sync} local={local} />
        </div>
      </header>

      <div role="tablist" className="tabs tabs-boxed mx-auto mt-4 max-w-3xl">
        <button
          type="button"
          role="tab"
          className={tab === 'local' ? 'tab tab-active' : 'tab'}
          onClick={() => setTab('local')}
        >
          Local (wasm)
        </button>
        <button
          type="button"
          role="tab"
          className={tab === 'server' ? 'tab tab-active' : 'tab'}
          onClick={() => setTab('server')}
        >
          Server (napi)
        </button>
        <button
          type="button"
          role="tab"
          className={tab === 'remote' ? 'tab tab-active' : 'tab'}
          onClick={() => setTab('remote')}
        >
          Remote (HTTP client)
        </button>
      </div>

      <main className="container mx-auto max-w-3xl flex-1 p-4 md:p-6">
        {local.kind === 'loading' && (
          <div className="flex justify-center py-16">
            <span className="loading loading-spinner loading-lg text-primary" />
          </div>
        )}
        {local.kind === 'error' && (
          <div role="alert" className="alert alert-error">
            <span>Worker failed: {local.message}</span>
          </div>
        )}
        {local.kind === 'ready' && tab === 'local' && (
          <LocalTab client={local.client} revision={revision.length} />
        )}
        {tab === 'server' && <ServerTab revision={revision.length} />}
        {tab === 'remote' && <RemoteTab />}
      </main>

      <SyncFooter sync={sync} />
    </div>
  );
}

function SyncBadge({
  sync,
  local,
}: {
  sync: ReturnType<typeof useSync>;
  local: ReturnType<typeof useLocalNotes>;
}) {
  if (local.kind === 'loading') {
    return <span className="badge badge-ghost">booting</span>;
  }
  if (local.kind === 'error') {
    return <span className="badge badge-error">error</span>;
  }
  return (
    <div className="flex items-center gap-2">
      {local.persistent ? (
        <span className="badge badge-success">OPFS</span>
      ) : (
        <span className="badge badge-warning">in-memory</span>
      )}
      {sync.online ? (
        <span className="badge badge-info">online</span>
      ) : (
        <span className="badge badge-error">offline</span>
      )}
      {sync.pending > 0 && (
        <span className="badge badge-warning">{sync.pending} pending</span>
      )}
    </div>
  );
}

function SyncFooter({ sync }: { sync: ReturnType<typeof useSync> }) {
  return (
    <footer className="bg-base-100 border-t border-base-300 px-4 py-3">
      <div className="container mx-auto flex max-w-3xl flex-wrap items-center justify-between gap-3 text-sm">
        <div className="flex items-center gap-3 text-base-content/70">
          <span>
            {sync.lastSyncAt
              ? `last sync · ${new Date(sync.lastSyncAt).toLocaleTimeString()}`
              : 'never synced'}
          </span>
          {sync.error && (
            <span className="text-error">⚠ {sync.error}</span>
          )}
        </div>
        <button
          type="button"
          className="btn btn-sm btn-primary"
          onClick={() => void sync.push()}
          disabled={sync.syncing || !sync.online}
        >
          {sync.syncing && <span className="loading loading-spinner loading-xs" />}
          Sync now
        </button>
      </div>
    </footer>
  );
}
