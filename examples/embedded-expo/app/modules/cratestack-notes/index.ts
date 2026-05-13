// Public TypeScript surface of the local cratestack-notes Expo module.
//
// The native side (ios/CratestackNotesModule.swift +
// android/.../CratestackNotesModule.kt) registers itself under the name
// "CratestackNotes" and exposes two async functions over the Expo
// modules bridge:
//
//   initDatabase(dbPath: string): Promise<void>
//   dispatch(requestJson: string): Promise<string>
//
// We wrap those in typed accessors that build/decode cratestack's JSON
// OperationRequest / OperationResponse envelope, so the React app
// programs against typed Note rows instead of raw JSON.

import { requireNativeModule } from 'expo-modules-core';

export type NoteView = {
  id: string;
  title: string;
  body: string;
  pinned: boolean;
  completed: boolean;
  createdAt: string;
  updatedAt: string;
};

export type NewNote = {
  title: string;
  body?: string;
  pinned?: boolean;
};

type OkEnvelope = { status: 'ok'; data: unknown };
type ErrEnvelope = { status: 'err'; code: string; message: string };
type Envelope = OkEnvelope | ErrEnvelope;

interface CratestackNotesNative {
  initDatabase(dbPath: string): Promise<void>;
  dispatch(requestJson: string): Promise<string>;
}

const native = requireNativeModule<CratestackNotesNative>('CratestackNotes');

function unwrap(envelope: Envelope): unknown {
  if (envelope.status === 'ok') return envelope.data;
  const err = new Error(envelope.message);
  (err as Error & { code: string }).code = envelope.code;
  throw err;
}

async function call<T>(
  model: string,
  kind: 'find_many' | 'find_unique' | 'create' | 'update' | 'delete',
  payload: unknown,
): Promise<T> {
  const requestJson = JSON.stringify({ model, kind, payload });
  const responseJson = await native.dispatch(requestJson);
  return unwrap(JSON.parse(responseJson) as Envelope) as T;
}

export async function initDatabase(dbPath: string): Promise<void> {
  await native.initDatabase(dbPath);
}

export function listNotes(): Promise<NoteView[]> {
  return call<NoteView[]>('Note', 'find_many', null);
}

export function findNote(id: string): Promise<NoteView | null> {
  return call<NoteView | null>('Note', 'find_unique', id);
}

export function createNote(input: NewNote): Promise<NoteView> {
  return call<NoteView>('Note', 'create', {
    title: input.title,
    body: input.body ?? '',
    pinned: input.pinned ?? false,
  });
}

export function updateNote(
  id: string,
  patch: Partial<Pick<NoteView, 'title' | 'body' | 'pinned' | 'completed'>>,
): Promise<NoteView> {
  return call<NoteView>('Note', 'update', { id, ...patch });
}

export function deleteNote(id: string): Promise<NoteView> {
  return call<NoteView>('Note', 'delete', id);
}
