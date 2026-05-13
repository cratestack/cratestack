import type { NextConfig } from 'next';
import withSerwistInit from '@serwist/next';

// @serwist/next is a webpack plugin — there's no Turbopack equivalent at
// the time of writing. Next.js 16 enables Turbopack by default, which
// means a bare `next dev` would silently skip the Serwist hooks and we'd
// get no service worker. The dev/build scripts in package.json pass
// `--webpack` explicitly to keep Serwist in the build graph.

const withSerwist = withSerwistInit({
  swSrc: 'app/sw.ts',
  swDest: 'public/sw.js',
  // Dev mode service worker registration can interfere with HMR; we keep it
  // off in dev and rely on next start / next build for the PWA story.
  disable: process.env.NODE_ENV === 'development',
});

const nextConfig: NextConfig = {
  // The napi addon is a native .node binary; tell Next not to try to bundle
  // it (it must be loaded with require() at runtime from disk).
  serverExternalPackages: ['react-nextjs-daisyui-napi'],
  // Cross-Origin Isolation lets OPFS-backed sqlite-wasm-rs use the SAH-pool
  // VFS reliably. Without these headers, some browsers refuse the worker's
  // SyncAccessHandle requests.
  async headers() {
    return [
      {
        source: '/(.*)',
        headers: [
          { key: 'Cross-Origin-Opener-Policy', value: 'same-origin' },
          { key: 'Cross-Origin-Embedder-Policy', value: 'require-corp' },
        ],
      },
    ];
  },
};

export default withSerwist(nextConfig);
