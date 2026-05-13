/// <reference lib="webworker" />
import { defaultCache } from '@serwist/next/worker';
import { Serwist } from 'serwist';
import type { PrecacheEntry, SerwistGlobalConfig } from 'serwist';

// Serwist's typed SW entry point. `swSrc` in `next.config.ts` points at this
// file and Serwist generates a production-ready `/public/sw.js`.
//
// `self.__SW_MANIFEST` is the asset manifest injected by Serwist at build
// time (HTML, JS, CSS, the wasm bundle, etc). `defaultCache` ships a
// reasonable runtime caching strategy: stale-while-revalidate for app code,
// cache-first for fonts/images, network-first for API responses.

declare global {
  interface WorkerGlobalScope extends SerwistGlobalConfig {
    __SW_MANIFEST: (PrecacheEntry | string)[] | undefined;
  }
}

declare const self: ServiceWorkerGlobalScope;

const serwist = new Serwist({
  precacheEntries: self.__SW_MANIFEST,
  skipWaiting: true,
  clientsClaim: true,
  navigationPreload: true,
  runtimeCaching: defaultCache,
});

serwist.addEventListeners();
