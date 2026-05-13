import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

// Cross-Origin Isolation is required for OPFS SharedArrayBuffer paths on some
// browsers. The wasm SAH-pool VFS itself doesn't strictly need it, but the
// headers keep the dev surface aligned with what a real PWA / Tauri shell
// would set, and prevents nasty surprises when you switch builds.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
    fs: {
      allow: ['..'],
    },
  },
  worker: {
    format: 'es',
  },
});
