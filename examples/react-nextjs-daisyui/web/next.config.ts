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

const NAPI_PACKAGE = 'react-nextjs-daisyui-napi';

const nextConfig: NextConfig = {
  // Tell Next's tracer to leave the napi addon out of the bundle — it's a
  // native .node binary that must be loaded with require() at runtime.
  serverExternalPackages: [NAPI_PACKAGE],

  // Defense in depth: under pnpm workspace symlinks `serverExternalPackages`
  // doesn't always catch the addon before webpack starts parsing it. The
  // webpack callback below makes the externalization explicit on the
  // server, and bans the client from resolving the package at all.
  webpack: (config, { isServer }) => {
    if (isServer) {
      const existing = Array.isArray(config.externals)
        ? config.externals
        : config.externals
          ? [config.externals]
          : [];
      // A string entry tells webpack "treat this import as a bare CJS
      // require() at runtime" — no resolution, no parse, no follow-through
      // into the .node binary.
      config.externals = [...existing, NAPI_PACKAGE];
    } else {
      // The addon is Node-only; aliasing to `false` makes any accidental
      // client-side import resolve to an empty module instead of pulling
      // the binary into the client bundle.
      config.resolve = config.resolve ?? {};
      config.resolve.alias = {
        ...(config.resolve.alias ?? {}),
        [NAPI_PACKAGE]: false,
      };
    }
    return config;
  },

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
