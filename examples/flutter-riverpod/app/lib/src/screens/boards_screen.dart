import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod_client/flutter_riverpod_client.dart';

import 'board_detail_screen.dart';

/// Lists boards via the generated `boardListProvider` and lets the user
/// add one via the generated `BoardCreateController`
/// (`boardCreateControllerProvider`) — every provider read/written here
/// comes from `client/`; this file only supplies `ConsumerWidget`/
/// `TextEditingController` glue, no `@riverpod` annotation anywhere.
///
/// `boardListProvider` now takes an optional `query` (issue #331), which
/// makes `riverpod_generator` emit it as a family — even this screen's
/// unfiltered, default-query usage has to call it, `boardListProvider()`,
/// rather than watch/invalidate the bare identifier.
class BoardsScreen extends ConsumerStatefulWidget {
  const BoardsScreen({super.key});

  @override
  ConsumerState<BoardsScreen> createState() => _BoardsScreenState();
}

class _BoardsScreenState extends ConsumerState<BoardsScreen> {
  final _nameController = TextEditingController();

  @override
  void dispose() {
    _nameController.dispose();
    super.dispose();
  }

  Future<void> _addBoard() async {
    final name = _nameController.text.trim();
    if (name.isEmpty) return;
    await ref.read(boardCreateControllerProvider.notifier).create(
          CreateBoardInput(
            id: DateTime.now().millisecondsSinceEpoch,
            name: name,
          ),
        );
    _nameController.clear();
    // The generated `get`/`list` providers are plain `Future` providers
    // with no built-in cache invalidation of their own (unlike the
    // TypeScript `swr` preset's fixed invalidation rule) — refreshing
    // after a write is ordinary Riverpod usage (`ref.invalidate` on an
    // already-generated provider), not a hand-written provider.
    ref.invalidate(boardListProvider());
  }

  @override
  Widget build(BuildContext context) {
    final boards = ref.watch(boardListProvider());
    final creating = ref.watch(boardCreateControllerProvider).isLoading;

    return Scaffold(
      appBar: AppBar(title: const Text('cratestack · flutter + riverpod preset')),
      body: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _nameController,
                    decoration: const InputDecoration(labelText: 'New board name'),
                    onSubmitted: (_) => _addBoard(),
                  ),
                ),
                const SizedBox(width: 8),
                FilledButton(
                  onPressed: creating ? null : _addBoard,
                  child: Text(creating ? 'Adding…' : 'Add board'),
                ),
              ],
            ),
            const SizedBox(height: 16),
            Expanded(
              child: boards.when(
                data: (items) => items.isEmpty
                    ? const Center(child: Text('No boards yet.'))
                    : ListView.builder(
                        itemCount: items.length,
                        itemBuilder: (context, index) {
                          final board = items[index];
                          return ListTile(
                            title: Text(board.name ?? '(untitled)'),
                            trailing: const Icon(Icons.chevron_right),
                            onTap: () => Navigator.of(context).push(
                              MaterialPageRoute(
                                builder: (_) => BoardDetailScreen(boardId: board.id!),
                              ),
                            ),
                          );
                        },
                      ),
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (error, _) => Center(child: Text('Failed to load boards: $error')),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
