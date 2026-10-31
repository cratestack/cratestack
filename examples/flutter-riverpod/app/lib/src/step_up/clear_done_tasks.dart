import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod_client/flutter_riverpod_client.dart';

import 'refusal_banner.dart';
import 'step_up_screen.dart';

/// "Clear done tasks", through the generated `ClearDoneTasksController`.
/// The procedure declares `@authn(maxAge: 300, amr: ["hwk"])`, so the demo's
/// password session is refused with a step-up first: [retryAfterStepUp]
/// reads it as [StepUpRequired], opens the [StepUpScreen], and sends the call
/// once more when the user confirmed. A declined step-up, or any other
/// refusal, shows a [RefusalBanner] keyed on its code.
class ClearDoneTasksButton extends ConsumerStatefulWidget {
  const ClearDoneTasksButton({super.key, required this.boardId});

  final int boardId;

  @override
  ConsumerState<ClearDoneTasksButton> createState() =>
      _ClearDoneTasksButtonState();
}

class _ClearDoneTasksButtonState extends ConsumerState<ClearDoneTasksButton> {
  Object? _error;
  int? _removed;

  Future<void> _clear() async {
    setState(() {
      _error = null;
      _removed = null;
    });
    final controller = ref.read(clearDoneTasksControllerProvider.notifier);
    final args = ClearDoneTasksProcedureArgs(
      args: ClearDoneTasksArgs(boardId: widget.boardId),
    );
    try {
      final result = await retryAfterStepUp(
        () => controller.clearDoneTasks(args),
        (stepUp) => StepUpScreen.ask(context, stepUp),
      );
      ref.invalidate(taskListProvider());
      if (mounted) setState(() => _removed = result.removed);
    } catch (error) {
      if (mounted) setState(() => _error = error);
    }
  }

  @override
  Widget build(BuildContext context) {
    // Keeps the generated controller alive across the call's await (the
    // auto-dispose race `board_detail_screen.dart` describes).
    final busy = ref.watch(clearDoneTasksControllerProvider).isLoading;
    final error = _error;
    final removed = _removed;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        OutlinedButton.icon(
          icon: const Icon(Icons.cleaning_services_outlined),
          label: Text(busy ? 'Clearing…' : 'Clear done tasks'),
          onPressed: busy ? null : _clear,
        ),
        if (removed != null)
          Text('Removed $removed done task${removed == 1 ? '' : 's'}.'),
        if (error != null) RefusalBanner(error: error, onRetry: _clear),
      ],
    );
  }
}
