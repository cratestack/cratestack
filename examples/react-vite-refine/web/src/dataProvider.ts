// Wires the CrateStack-generated REST client to `@cratestack/refine`'s
// data provider using the GENERATED manifest (`cratestackRefineResources`,
// from `--refine`), not a hand-written one — the whole point of this
// example (see README.md). `generated/` is produced by
// `just react-vite-refine-fixture` and is gitignored; `predev`/
// `prebuild`/`pretypecheck` (package.json) fail loudly if it's missing.
//
// Imports go straight to `client.js`/`refine.js`, deliberately skipping
// the generated package's own `index.ts` barrel: that barrel re-exports
// `react-query.js`, which imports `@tanstack/react-query` — a real
// dependency this app has no other reason to install (it uses
// `@refinedev/core`'s own hooks, not the generated package's
// TanStack Query wrappers). Importing the two files this app actually
// needs keeps that dependency out of `package.json` entirely.
import { createCratestackDataProvider } from "@cratestack/refine";
import { ReactViteRefineClientClient } from "../generated/src/client.js";
import { cratestackRefineResources } from "../generated/src/refine.js";

// `window.location.origin`, not a blank string: the generated runtime
// builds every request URL via `new URL(path, `${origin}/`)`
// (`generated/src/runtime.ts`), and the WHATWG URL constructor rejects a
// bare "/" as an invalid base — confirmed by hand, it throws
// `TypeError: Invalid base URL` at the first request. Using the page's
// own origin makes every request same-origin, routed through Vite dev
// server's `/api` proxy (vite.config.ts) to the WireMock container —
// required because the generated stubs have no CORS/OPTIONS handling
// (see README.md's "What this demo can't prove"): a direct cross-origin
// `fetch("http://localhost:8080/...")` from the browser fails its
// preflight (confirmed by hand too — WireMock 404s the OPTIONS request).
// Override with `VITE_CRATESTACK_API_URL` for a setup that fronts the
// mock with its own CORS-aware reverse proxy instead.
export const CRATESTACK_API_URL = import.meta.env.VITE_CRATESTACK_API_URL ?? window.location.origin;

export const client = new ReactViteRefineClientClient(CRATESTACK_API_URL, {
  basePath: "/api",
});

export const dataProvider = createCratestackDataProvider(cratestackRefineResources(client));
