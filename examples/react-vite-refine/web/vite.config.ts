import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The generated WireMock stubs have no CORS handling at all (confirmed —
// see README.md's "What this demo can't prove": the generator emits no
// OPTIONS stub, so a browser's cross-origin preflight against
// `localhost:8080` 404s). Proxying `/api` to the container here makes
// every request same-origin from the browser's point of view instead —
// no change to `cratestack-mock-wiremock` needed. `src/dataProvider.ts`
// points the generated client at the page's own origin to go through
// this. `8080` is not overridable here (no `process.env` — this file has
// no `@types/node` and stays out of that dependency on purpose); it's
// the port every doc/script in this example hardcodes for the container.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": {
        target: "http://localhost:8080",
        changeOrigin: true,
      },
    },
  },
});
