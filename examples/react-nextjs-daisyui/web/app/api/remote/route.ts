import { NextResponse } from 'next/server';
import { loadAddon } from '../addon';

// Remote tab: fan out to an upstream CrateStack service over the typed
// HTTP client living inside the napi addon. The browser never sees the
// upstream URL — it goes through Next.js to the addon, which holds any
// outbound TLS material, headers, or service-token rotation logic.
//
// Query usage: `/api/remote?url=https%3A%2F%2Fapi.example.com`. In a real
// deployment you'd pull this from an env var on the server and never let
// the client choose it; we accept it here purely so the example demo can
// be wired up against any running service.

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';

export async function GET(request: Request) {
  try {
    const url = new URL(request.url);
    const baseUrl = url.searchParams.get('url');
    if (!baseUrl) {
      return NextResponse.json(
        { error: 'missing ?url query parameter' },
        { status: 400 },
      );
    }
    const addon = loadAddon();
    const articles = await addon.fetchRemoteArticles(baseUrl);
    return NextResponse.json({ articles });
  } catch (error) {
    return NextResponse.json(
      { error: error instanceof Error ? error.message : String(error) },
      { status: 502 },
    );
  }
}
