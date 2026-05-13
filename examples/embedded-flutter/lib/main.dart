// Flutter UI for the cratestack embedded-flutter example.
//
// The generated bindings live under `lib/src/rust/`. They appear after
// running `dart run flutter_rust_bridge_codegen generate` from the
// example root, fed by `flutter_rust_bridge.yaml`. Until you run the
// generator, the imports below will be red — that's expected.

import 'package:flutter/material.dart';
import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';

import 'src/rust/api/notes.dart';
import 'src/rust/frb_generated.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // Boot the Rust runtime that owns the generated bindings. Without
  // this the api functions can't dispatch into the native lib.
  await RustLib.init();

  // Resolve a writable SQLite location on every platform. On iOS that
  // ends up under the app's Documents dir; on Android it's the
  // app-private data dir; on desktops the platform's application
  // support directory.
  final Directory dir = await getApplicationSupportDirectory();
  final dbPath = p.join(dir.path, 'cratestack-notes.db');
  await initDatabase(dbPath: dbPath);

  runApp(const CratestackApp());
}

class CratestackApp extends StatelessWidget {
  const CratestackApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'cratestack · embedded-flutter',
      theme: ThemeData.from(
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xFF0F766E),
          brightness: Brightness.light,
        ),
        useMaterial3: true,
      ),
      home: const NotesScreen(),
    );
  }
}

class NotesScreen extends StatefulWidget {
  const NotesScreen({super.key});

  @override
  State<NotesScreen> createState() => _NotesScreenState();
}

class _NotesScreenState extends State<NotesScreen> {
  List<NoteView> _notes = const <NoteView>[];
  bool _onlyOpen = false;
  bool _loading = false;
  String? _error;

  final TextEditingController _titleCtrl = TextEditingController();
  bool _pinned = false;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  @override
  void dispose() {
    _titleCtrl.dispose();
    super.dispose();
  }

  Future<void> _refresh() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final rows = await listNotes(onlyOpen: _onlyOpen);
      setState(() => _notes = rows);
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      setState(() => _loading = false);
    }
  }

  Future<void> _submit() async {
    final title = _titleCtrl.text.trim();
    if (title.isEmpty) return;
    try {
      await addNote(
        input: NewNote(title: title, body: '', pinned: _pinned),
      );
      _titleCtrl.clear();
      setState(() => _pinned = false);
      await _refresh();
    } catch (e) {
      setState(() => _error = e.toString());
    }
  }

  Future<void> _markDone(NoteView note) async {
    try {
      await markDone(id: note.id);
      await _refresh();
    } catch (e) {
      setState(() => _error = e.toString());
    }
  }

  Future<void> _delete(NoteView note) async {
    try {
      await deleteNote(id: note.id);
      await _refresh();
    } catch (e) {
      setState(() => _error = e.toString());
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('cratestack · embedded-flutter'),
        actions: [
          Row(
            children: [
              const Text('hide done'),
              Switch(
                value: _onlyOpen,
                onChanged: (v) {
                  setState(() => _onlyOpen = v);
                  _refresh();
                },
              ),
              const SizedBox(width: 8),
            ],
          ),
        ],
      ),
      body: Column(
        children: [
          if (_error != null)
            Container(
              width: double.infinity,
              color: Theme.of(context).colorScheme.errorContainer,
              padding: const EdgeInsets.all(8),
              child: Text(
                _error!,
                style: TextStyle(
                  color: Theme.of(context).colorScheme.onErrorContainer,
                ),
              ),
            ),
          Padding(
            padding: const EdgeInsets.all(12),
            child: Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _titleCtrl,
                    decoration: const InputDecoration(
                      labelText: 'New note title',
                      border: OutlineInputBorder(),
                    ),
                    onSubmitted: (_) => _submit(),
                  ),
                ),
                const SizedBox(width: 8),
                Checkbox(
                  value: _pinned,
                  onChanged: (v) => setState(() => _pinned = v ?? false),
                ),
                const Text('pin'),
                const SizedBox(width: 8),
                FilledButton(onPressed: _submit, child: const Text('Add')),
              ],
            ),
          ),
          Expanded(
            child: _loading
                ? const Center(child: CircularProgressIndicator())
                : _notes.isEmpty
                    ? const Center(
                        child: Text('No notes yet — add one above.'),
                      )
                    : ListView.builder(
                        itemCount: _notes.length,
                        itemBuilder: (context, index) {
                          final note = _notes[index];
                          return ListTile(
                            leading: note.pinned
                                ? const Icon(Icons.push_pin, size: 18)
                                : null,
                            title: Text(
                              note.title,
                              style: TextStyle(
                                decoration: note.completed
                                    ? TextDecoration.lineThrough
                                    : null,
                              ),
                            ),
                            subtitle: Text(
                              'updated ${DateTime.parse(note.updatedAt).toLocal()}',
                            ),
                            trailing: Row(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                if (!note.completed)
                                  IconButton(
                                    tooltip: 'Mark done',
                                    icon: const Icon(Icons.check),
                                    onPressed: () => _markDone(note),
                                  ),
                                IconButton(
                                  tooltip: 'Delete',
                                  icon: const Icon(Icons.delete_outline),
                                  onPressed: () => _delete(note),
                                ),
                              ],
                            ),
                          );
                        },
                      ),
          ),
        ],
      ),
    );
  }
}
