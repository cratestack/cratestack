import { defineConfig } from 'vite';

// No wasm, no Worker, no special headers needed. This renderer is a
// pure view layer — every data operation, local and remote, goes
// through `@tauri-apps/api/core`'s `invoke()` to the native shell.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
});
