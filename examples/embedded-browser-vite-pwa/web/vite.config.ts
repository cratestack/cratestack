import { defineConfig } from 'vite';
import { VitePWA } from 'vite-plugin-pwa';

// Installable PWA on top of the embedded-browser-vite layout.
//
// vite-plugin-pwa generates the service worker (via Workbox) at build time:
// it precaches the page shell + every emitted JS/CSS asset + the wasm bundle
// so the app stays usable on a cold cache and continues to function with the
// network unplugged. Data is already offline-first thanks to OPFS — this
// extends that to the app shell itself.

export default defineConfig({
  plugins: [
    VitePWA({
      registerType: 'autoUpdate',
      injectRegister: 'auto',
      devOptions: {
        enabled: true,
        type: 'module',
      },
      workbox: {
        // Precache the page shell + every emitted asset, including the
        // wasm chunk, so the app boots offline.
        globPatterns: ['**/*.{js,css,html,wasm,svg}'],
        // Raise the per-asset size budget so the ~960 KiB wasm fits
        // (Workbox default is 2 MiB which already covers us; be explicit).
        maximumFileSizeToCacheInBytes: 3 * 1024 * 1024,
      },
      manifest: {
        name: 'cratestack notes',
        short_name: 'cratestack',
        description:
          'Offline-first notes app powered by cratestack-rusqlite + OPFS, packaged as a PWA.',
        theme_color: '#0f766e',
        background_color: '#ffffff',
        display: 'standalone',
        start_url: '/',
        icons: [
          {
            src: '/icon.svg',
            sizes: 'any',
            type: 'image/svg+xml',
            purpose: 'any maskable',
          },
        ],
      },
    }),
  ],
  server: {
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
  },
  worker: {
    format: 'es',
  },
  optimizeDeps: {
    exclude: ['./pkg/embedded_browser_vite_pwa_example.js'],
  },
});
