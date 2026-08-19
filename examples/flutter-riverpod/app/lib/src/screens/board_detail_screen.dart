import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod_client/flutter_riverpod_client.dart';

/// One pomodoro — the `estimateFocusMinutes` procedure's own unit,
/// matching the `react-vite-swr` example's `MINUTES_PER_TASK` constant
/// so the two sibling examples agree on the demo's numbers.
const _minutesPerTask = 25;

/// Detail screen for one board: its tasks (generated `taskListProvider`,
/// filtered client-side by `boardId`) plus a focus-time estimate from
/// the generated `estimateFocusMinutesProvider` procedure provider.
///
/// `taskListProvider`/`taskListProvider` has no server-side "tasks on
/// this board" filter wired up yet — the same known preset gap
/// `react-vite-swr/README.md` documents for its TypeScript sibling — so
/// this reads the full task list (still 100% generated) and filters
/// client-side, exactly like `BoardDetailScreen.tsx` does. It could
/// filter server-side via `taskListProvider(query: CratestackListQuery(where: ...))`
/// (issue #331) instead — left as client-side filtering here to keep
/// this diff scoped to the call-syntax change the new optional `query`
/// parameter forces (see below), not a rewrite of this demo's data flow.
///
/// `taskListProvider` now takes an optional `query` (issue #331), which
/// makes `riverpod_generator` emit it as a family — even this screen's
/// unfiltered, default-query usage has to call it, `taskListProvider()`,
/// rather than watch/invalidate the bare identifier.
///
/// Every provider read/written here comes from `client/`: `board(id)`,
/// `taskList`, `TaskCreateController`, `TaskUpdateController`,
/// `TaskDeleteController`, `estimateFocusMinutes`. Zero hand-written
/// providers.
class BoardDetailScreen extends ConsumerStatefulWidget {
  const BoardDetailScreen({super.key, required this.boardId});

  final int boardId;

  @override
  ConsumerState<BoardDetailScreen> createState() => _BoardDetailScreenState();
}

class _BoardDetailScreenState extends ConsumerState<BoardDetailScreen> {
  final _titleController = TextEditingController();

  @override
  void dispose() {
    _titleController.dispose();
    super.dispose();
  }

  Future<void> _addTask() async {
    final title = _titleController.text.trim();
    if (title.isEmpty) return;
    // Deliberately the named constructor, not `CreateTaskInputBuilder()`.
    // Every field here is required, and Dart already checks `required`
    // named arguments at compile time — the builder's `build()` can only
    // check at runtime, so on a create input the constructor is the
    // *stronger* of the two. (Rust is the opposite way round: there the
    // builder's typestate is the compile-time check and the struct literal
    // is what a new field silently breaks.) See `_toggleDone` below for the
    // shape where the Dart builder does earn its place.
    await ref.read(taskCreateControllerProvider.notifier).create(
          CreateTaskInput(
            id: DateTime.now().millisecondsSinceEpoch,
            title: title,
            done: false,
            boardId: widget.boardId,
          ),
        );
    _titleController.clear();
    ref.invalidate(taskListProvider());
  }

  Future<void> _toggleDone(Task task) async {
    // `TaskUpdateController` is a single global controller (its `save`
    // method takes the target `id` as an argument rather than the
    // provider being keyed by id) — see `model_providers.dart.j2`'s own
    // header comment. Fine for this demo's scale; a larger app might
    // want a per-row wrapper, same tradeoff `TaskRow.tsx` notes for the
    // TypeScript sibling's per-row hook binding.
    // The builder pays off here: an update input is all-optional, so the
    // named constructor offers no compile-time guarantee to give up, and
    // `.done(..)` reads as "patch exactly this field" where a constructor
    // call with one argument reads as "every other field is null".
    await ref.read(taskUpdateControllerProvider.notifier).save(
          task.id!,
          UpdateTaskInputBuilder().done(!(task.done ?? false)).build(),
        );
    ref.invalidate(taskListProvider());
  }

  Future<void> _deleteTask(Task task) async {
    await ref.read(taskDeleteControllerProvider.notifier).delete(task.id!);
    ref.invalidate(taskListProvider());
  }

  @override
  Widget build(BuildContext context) {
    final board = ref.watch(boardProvider(widget.boardId));
    final allTasks = ref.watch(taskListProvider());
    final creating = ref.watch(taskCreateControllerProvider).isLoading;
    // Real bug found running this app against a live server (issue #303):
    // `TaskUpdateController`/`TaskDeleteController` are `@riverpod`
    // AsyncNotifier controllers, which auto-dispose the moment nothing
    // is watching them. `_toggleDone`/`_deleteTask` below only ever
    // `ref.read(...).notifier` them — no watch — so the controller could
    // get garbage-collected *during* its own `save`/`delete` call's
    // network await, and the generated controller's `state = ...`
    // afterward throws "Cannot use the Ref of ... after it has been
    // disposed." (reproduced live, not hypothesized). `ref.watch` here
    // keeps both alive for this screen's lifetime — the same pattern
    // `taskCreateControllerProvider` above already (correctly) uses —
    // this app just forgot to apply it to update/delete too.
    ref.watch(taskUpdateControllerProvider);
    ref.watch(taskDeleteControllerProvider);

    return Scaffold(
      // Riverpod 3.x note: `AsyncValue.value` is nullable directly now —
      // the 2.x `valueOrNull` extension getter this line originally used
      // (muscle memory from Riverpod 2.x docs/examples) doesn't exist on
      // this pinned Riverpod 3.3.2 and is a real `flutter analyze`
      // `undefined_getter` error (confirmed empirically).
      appBar: AppBar(title: Text(board.value?.name ?? 'Board')),
      body: Padding(
        padding: const EdgeInsets.all(16),
        child: allTasks.when(
          data: (tasks) {
            final boardTasks =
                tasks.where((task) => task.boardId == widget.boardId).toList();
            final openCount =
                boardTasks.where((task) => !(task.done ?? false)).length;
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                // Constructed fresh on every rebuild — deliberately not
                // memoized. Issue #325 fixed the real bug that used to
                // force a memoization workaround here (see this file's
                // `git log`/the linked issue for the removed code): with
                // `dart_mappable`-generated `operator ==`/`hashCode` on
                // `EstimateFocusMinutesArgs`, a brand-new instance with
                // the same field values is `==` to the last one, so
                // riverpod's family provider cache still dedupes it
                // correctly and `estimateFocusMinutesProvider` resolves
                // instead of restarting from `AsyncLoading` on every
                // rebuild.
                _FocusEstimate(
                  openCount: openCount,
                  args: EstimateFocusMinutesArgs(
                    args: FocusEstimateArgs(
                      taskCount: openCount,
                      minutesPerTask: _minutesPerTask,
                    ),
                  ),
                ),
                const SizedBox(height: 8),
                Row(
                  children: [
                    Expanded(
                      child: TextField(
                        controller: _titleController,
                        decoration:
                            const InputDecoration(labelText: 'New task title'),
                        onSubmitted: (_) => _addTask(),
                      ),
                    ),
                    const SizedBox(width: 8),
                    FilledButton(
                      onPressed: creating ? null : _addTask,
                      child: Text(creating ? 'Adding…' : 'Add task'),
                    ),
                  ],
                ),
                const SizedBox(height: 16),
                Expanded(
                  child: boardTasks.isEmpty
                      ? const Center(
                          child: Text('No tasks on this board yet.'))
                      : ListView.builder(
                          itemCount: boardTasks.length,
                          itemBuilder: (context, index) {
                            final task = boardTasks[index];
                            return CheckboxListTile(
                              value: task.done ?? false,
                              onChanged: (_) => _toggleDone(task),
                              title: Text(
                                task.title ?? '(untitled)',
                                style: (task.done ?? false)
                                    ? const TextStyle(
                                        decoration:
                                            TextDecoration.lineThrough)
                                    : null,
                              ),
                              secondary: IconButton(
                                icon: const Icon(Icons.delete_outline),
                                onPressed: () => _deleteTask(task),
                              ),
                            );
                          },
                        ),
                ),
              ],
            );
          },
          loading: () => const Center(child: CircularProgressIndicator()),
          error: (error, _) => Center(child: Text('Failed to load tasks: $error')),
        ),
      ),
    );
  }
}

class _FocusEstimate extends ConsumerWidget {
  const _FocusEstimate({required this.openCount, required this.args});

  final int openCount;
  final EstimateFocusMinutesArgs args;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final estimate = ref.watch(estimateFocusMinutesProvider(args));
    return estimate.when(
      data: (result) => Text(
        '$openCount open task${openCount == 1 ? '' : 's'} ≈ '
        '${result.totalMinutes} focus minutes ($_minutesPerTask min/task, '
        'via the estimateFocusMinutes procedure provider)',
        style: Theme.of(context).textTheme.bodySmall,
      ),
      loading: () => const SizedBox.shrink(),
      error: (error, _) => Text('Estimate failed: $error'),
    );
  }
}
