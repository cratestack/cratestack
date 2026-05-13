import { App } from '@/components/App';

// The whole UI lives in a client component because the wasm/OPFS layer
// depends on browser globals (Worker, OPFS, navigator.onLine). The page
// route itself is server-rendered as a thin shell.

export default function Page() {
  return <App />;
}
