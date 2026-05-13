import { NextResponse } from 'next/server';
import { loadAddon } from '../addon';

// Server-owned notes — backed by the napi addon's SQLite file. Reads the
// full list, ordered by updatedAt desc.
//
// `force-dynamic` because Next.js will otherwise try to cache the response
// at build time. The data is per-process and the source of truth for the
// "server-side embedded" tab.

export const dynamic = 'force-dynamic';
export const runtime = 'nodejs';

export async function GET() {
  try {
    const addon = loadAddon();
    return NextResponse.json({ notes: addon.listNotes() });
  } catch (error) {
    return NextResponse.json(
      { error: error instanceof Error ? error.message : String(error) },
      { status: 500 },
    );
  }
}

export async function POST(request: Request) {
  try {
    const body = (await request.json()) as {
      title?: string;
      body?: string;
      pinned?: boolean;
    };
    if (!body.title) {
      return NextResponse.json({ error: 'title required' }, { status: 400 });
    }
    const addon = loadAddon();
    const now = new Date().toISOString();
    const note = addon.upsertNote({
      id: crypto.randomUUID(),
      title: body.title,
      body: body.body ?? '',
      pinned: Boolean(body.pinned),
      completed: false,
      createdAt: now,
      updatedAt: now,
    });
    return NextResponse.json({ note });
  } catch (error) {
    return NextResponse.json(
      { error: error instanceof Error ? error.message : String(error) },
      { status: 500 },
    );
  }
}
