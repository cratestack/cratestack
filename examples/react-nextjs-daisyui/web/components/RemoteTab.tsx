'use client';

import { useCallback, useState } from 'react';
import type { FormEvent } from 'react';

// "Remote" tab — fans out to an upstream CrateStack service via the typed
// HTTP client living inside the napi addon. The browser hands a base URL
// to /api/remote and the Next.js Route Handler does the actual HTTP call.

type ArticleRow = {
  id: number;
  title: string;
  body: string;
  published: boolean;
  createdAt: string;
};

export function RemoteTab() {
  const [articles, setArticles] = useState<ArticleRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [url, setUrl] = useState<string>('http://localhost:8080');

  const onSubmit = useCallback(
    async (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      setLoading(true);
      setError(null);
      try {
        const response = await fetch(
          `/api/remote?url=${encodeURIComponent(url)}`,
          { cache: 'no-store' },
        );
        if (!response.ok) {
          const data = (await response.json().catch(() => ({}))) as {
            error?: string;
          };
          throw new Error(data.error ?? `fetch failed: ${response.status}`);
        }
        const data = (await response.json()) as { articles: ArticleRow[] };
        setArticles(data.articles);
      } catch (raised) {
        setError(raised instanceof Error ? raised.message : String(raised));
      } finally {
        setLoading(false);
      }
    },
    [url],
  );

  return (
    <div className="space-y-6">
      <div className="card bg-base-100 shadow">
        <div className="card-body">
          <h2 className="card-title">Call upstream service</h2>
          <p className="text-sm text-base-content/70">
            The browser submits a URL; Next.js Route Handler delegates to
            the napi addon's typed{' '}
            <code className="kbd kbd-sm">include_client_schema!</code>
            -generated client. Browser never speaks to the upstream
            directly — CORS and credentials stay server-side.
          </p>
          <form onSubmit={onSubmit} className="mt-2 flex flex-col gap-3 sm:flex-row">
            <input
              type="url"
              className="input input-bordered flex-1"
              required
              placeholder="https://articles.example.com"
              value={url}
              onChange={(event) => setUrl(event.target.value)}
            />
            <button type="submit" className="btn btn-accent" disabled={loading}>
              {loading && <span className="loading loading-spinner loading-xs" />}
              Fetch Articles
            </button>
          </form>
        </div>
      </div>

      {error && (
        <div role="alert" className="alert alert-error">
          <span>{error}</span>
        </div>
      )}

      <div className="card bg-base-100 shadow">
        <div className="card-body">
          <h2 className="card-title">
            Articles
            <span className="badge badge-neutral">{articles.length}</span>
          </h2>
          {articles.length === 0 ? (
            <p className="text-base-content/60 italic">
              No articles yet. Point this at a running CrateStack service
              exposing an Article model and click Fetch.
            </p>
          ) : (
            <ul className="space-y-2">
              {articles.map((article) => (
                <li
                  key={article.id}
                  className="note-row rounded-lg border border-base-300 p-3"
                >
                  <div className="flex items-center gap-2">
                    {article.published ? (
                      <span className="badge badge-success badge-sm">published</span>
                    ) : (
                      <span className="badge badge-ghost badge-sm">draft</span>
                    )}
                    <h3 className="font-semibold">{article.title}</h3>
                  </div>
                  {article.body && (
                    <p className="mt-1 whitespace-pre-wrap text-sm text-base-content/80">
                      {article.body}
                    </p>
                  )}
                  <p className="mt-1 text-xs text-base-content/50">
                    {new Date(article.createdAt).toLocaleString()}
                  </p>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
