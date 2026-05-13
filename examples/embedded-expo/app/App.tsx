// Expo + React Native UI for the cratestack embedded-expo example.
//
// Every data operation hits the `cratestack-notes` local module, which
// routes a JSON envelope to the `embedded_expo_native` Rust cdylib. No
// SQL, no networking, no plain-JS storage — same shape as every other
// embedded example, just driven through the React Native bridge.

import { useCallback, useEffect, useState } from 'react';
import {
  ActivityIndicator,
  Alert,
  FlatList,
  Pressable,
  StyleSheet,
  Switch,
  Text,
  TextInput,
  View,
} from 'react-native';
import * as FileSystem from 'expo-file-system';
import {
  createNote,
  deleteNote,
  initDatabase,
  listNotes,
  updateNote,
  type NoteView,
} from 'cratestack-notes';

// "Mark done" is just `updateNote(id, { completed: true })`; pulled out
// here so each UI handler reads as one verb.
const markDone = (id: string) => updateNote(id, { completed: true });

export default function App() {
  const [notes, setNotes] = useState<NoteView[]>([]);
  const [title, setTitle] = useState('');
  const [pinned, setPinned] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [booting, setBooting] = useState(true);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      setNotes(await listNotes());
    } catch (raised) {
      setError(raised instanceof Error ? raised.message : String(raised));
    }
  }, []);

  useEffect(() => {
    (async () => {
      try {
        // Expo's documentDirectory is per-app sandbox; on iOS it's
        // ~/Documents, on Android the app's internal storage.
        const dir = FileSystem.documentDirectory ?? '';
        const path = `${dir}cratestack-notes.db`;
        await initDatabase(path);
        await refresh();
      } catch (raised) {
        setError(raised instanceof Error ? raised.message : String(raised));
      } finally {
        setBooting(false);
      }
    })();
  }, [refresh]);

  const submit = useCallback(async () => {
    if (!title.trim()) return;
    try {
      setError(null);
      await createNote({ title: title.trim(), body: '', pinned });
      setTitle('');
      setPinned(false);
      await refresh();
    } catch (raised) {
      setError(raised instanceof Error ? raised.message : String(raised));
    }
  }, [title, pinned, refresh]);

  const onMarkDone = useCallback(
    async (note: NoteView) => {
      try {
        await markDone(note.id);
        await refresh();
      } catch (raised) {
        setError(raised instanceof Error ? raised.message : String(raised));
      }
    },
    [refresh],
  );

  const onDelete = useCallback(
    async (note: NoteView) => {
      Alert.alert('Delete note', `Delete "${note.title}"?`, [
        { text: 'Cancel', style: 'cancel' },
        {
          text: 'Delete',
          style: 'destructive',
          onPress: async () => {
            try {
              await deleteNote(note.id);
              await refresh();
            } catch (raised) {
              setError(raised instanceof Error ? raised.message : String(raised));
            }
          },
        },
      ]);
    },
    [refresh],
  );

  if (booting) {
    return (
      <View style={styles.center}>
        <ActivityIndicator size="large" />
      </View>
    );
  }

  return (
    <View style={styles.container}>
      <Text style={styles.heading}>cratestack · embedded-expo</Text>
      {error ? <Text style={styles.error}>{error}</Text> : null}
      <View style={styles.form}>
        <TextInput
          style={styles.input}
          placeholder="New note title"
          value={title}
          onChangeText={setTitle}
          onSubmitEditing={submit}
          returnKeyType="done"
        />
        <View style={styles.formRow}>
          <Switch value={pinned} onValueChange={setPinned} />
          <Text style={styles.label}>pin</Text>
          <Pressable style={styles.addButton} onPress={submit}>
            <Text style={styles.addButtonText}>Add</Text>
          </Pressable>
        </View>
      </View>
      <FlatList
        data={notes}
        keyExtractor={(item) => item.id}
        renderItem={({ item }) => (
          <View style={styles.note}>
            <View style={styles.noteHead}>
              {item.pinned ? <Text style={styles.pin}>📌</Text> : null}
              <Text
                style={[
                  styles.noteTitle,
                  item.completed ? styles.noteTitleDone : null,
                ]}
              >
                {item.title}
              </Text>
            </View>
            <Text style={styles.noteMeta}>
              updated {new Date(item.updatedAt).toLocaleString()}
            </Text>
            <View style={styles.noteActions}>
              {!item.completed ? (
                <Pressable onPress={() => onMarkDone(item)}>
                  <Text style={styles.action}>Done</Text>
                </Pressable>
              ) : null}
              <Pressable onPress={() => onDelete(item)}>
                <Text style={styles.actionDanger}>Delete</Text>
              </Pressable>
            </View>
          </View>
        )}
        ListEmptyComponent={
          <Text style={styles.empty}>No notes yet — add one above.</Text>
        }
      />
    </View>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, padding: 16, paddingTop: 60, backgroundColor: '#0f172a' },
  center: { flex: 1, alignItems: 'center', justifyContent: 'center' },
  heading: { color: '#f8fafc', fontSize: 18, fontWeight: '700', marginBottom: 12 },
  error: {
    backgroundColor: '#7f1d1d',
    color: '#fee2e2',
    padding: 8,
    borderRadius: 4,
    marginBottom: 8,
  },
  form: { marginBottom: 16 },
  input: {
    backgroundColor: '#1e293b',
    color: '#f8fafc',
    paddingHorizontal: 12,
    paddingVertical: 10,
    borderRadius: 6,
    fontSize: 16,
  },
  formRow: { flexDirection: 'row', alignItems: 'center', marginTop: 8, gap: 8 },
  label: { color: '#94a3b8' },
  addButton: {
    marginLeft: 'auto',
    backgroundColor: '#14b8a6',
    paddingHorizontal: 14,
    paddingVertical: 8,
    borderRadius: 6,
  },
  addButtonText: { color: '#0f172a', fontWeight: '700' },
  note: {
    padding: 12,
    borderRadius: 8,
    backgroundColor: '#1e293b',
    marginBottom: 8,
  },
  noteHead: { flexDirection: 'row', alignItems: 'center', gap: 6 },
  pin: { fontSize: 14 },
  noteTitle: { color: '#f8fafc', fontSize: 16, fontWeight: '600' },
  noteTitleDone: { textDecorationLine: 'line-through', opacity: 0.6 },
  noteMeta: { color: '#94a3b8', fontSize: 12, marginTop: 4 },
  noteActions: { flexDirection: 'row', gap: 16, marginTop: 8 },
  action: { color: '#34d399', fontWeight: '600' },
  actionDanger: { color: '#fca5a5', fontWeight: '600' },
  empty: { color: '#94a3b8', fontStyle: 'italic', textAlign: 'center', marginTop: 24 },
});
