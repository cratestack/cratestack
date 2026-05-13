import type { NextConfig } from 'next';
import withSerwistInit from '@serwist/next';

const withSerwist = withSerwistInit({
  swSrc: 'app/sw.ts',
  swDest: 'public/sw.js',
  // Dev mode service worker registration can interfere with HMR; we keep it
  // off in dev and rely on next start / next build for the PWA story.
  disable: process.env.NODE_ENV === 'development',
});

const nextConfig: NextConfig = {
  // Required for `new Worker(new URL('./worker.ts', import.meta.url))` to
  // bundle correctly under Turbopack and webpack 5.
  experimental: {
    // Keep React 19 Server Actions on (default in Next 16, here for clarity).
  },
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
