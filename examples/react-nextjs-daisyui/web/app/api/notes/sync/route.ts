import { NextResponse } from 'next/server';
import { loadAddon, type NoteRow } from '../../addon';

// Offline-first sync endpoint.
//
// Wire protocol (POST):
//   request:  { cursor: string;  pushes: NoteRow[] }
//   response: { cursor: string;  remote: NoteRow[] }
//
// `cursor` is the RFC3339 timestamp of the last successful pull. The
// server returns rows whose updatedAt is strictly newer than the cursor,
// AND echoes back a new cursor (server `Date.now()`-shaped RFC3339) to
// use on the next round-trip.
//
// `pushes` are local rows the client wants reconciled. The server upserts
// each via napi (last-write-wins by updatedAt). If the server's copy is
// newer, the upsert is a no-op and the next pull will deliver the diff.

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';

type SyncRequest = {
  cursor?: string;
  pushes?: NoteRow[];
};

export async function POST(request: Request) {
  try {
    const body = (await request.json()) as SyncRequest;
    const addon = loadAddon();
    const pushes = Array.isArray(body.pushes) ? body.pushes : [];
    for (const note of pushes) {
      addon.upsertNote(note);
    }
    const cursor = typeof body.cursor === 'string' ? body.cursor : '';
    const remote = addon.notesSince(cursor);
    return NextResponse.json({
      cursor: new Date().toISOString(),
      remote,
      pushed: pushes.length,
    });
  } catch (error) {
    return NextResponse.json(
      { error: error instanceof Error ? error.message : String(error) },
      { status: 500 },
    );
  }
}
