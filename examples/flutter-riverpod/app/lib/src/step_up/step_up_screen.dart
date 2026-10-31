import 'package:flutter/material.dart';
import 'package:flutter_riverpod_client/flutter_riverpod_client.dart';

import '../session.dart';

/// The step-up screen: what the refused operation requires, read from the
/// generated [StepUpRequired], and the re-proof. It pops `true` when the user
/// confirmed and `false` when they declined, which is what
/// [retryAfterStepUp]'s `reprove` answers.
class StepUpScreen extends StatelessWidget {
  const StepUpScreen({super.key, required this.stepUp});

  final StepUpRequired stepUp;

  /// Shows the screen over [context] and answers whether the user re-proved.
  static Future<bool> ask(BuildContext context, StepUpRequired stepUp) async {
    final proved = await Navigator.of(context).push<bool>(
      MaterialPageRoute(
        fullscreenDialog: true,
        builder: (_) => StepUpScreen(stepUp: stepUp),
      ),
    );
    return proved ?? false;
  }

  @override
  Widget build(BuildContext context) {
    final maxAge = stepUp.maxAge;
    return Scaffold(
      appBar: AppBar(title: const Text('Confirm it is you')),
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Text(
              'This action needs a fresh sign-in with your security key.',
              style: TextStyle(fontSize: 18),
            ),
            const SizedBox(height: 16),
            if (maxAge != null)
              Text('Signed in within the last ${maxAge ~/ 60} minutes'),
            if (stepUp.amr.isNotEmpty) Text('With: ${stepUp.amr.join(', ')}'),
            if (stepUp.unmet.isNotEmpty)
              Text('Missing now: ${stepUp.unmet.join(', ')}'),
            const SizedBox(height: 24),
            FilledButton.icon(
              icon: const Icon(Icons.key),
              label: const Text('Confirm with security key'),
              onPressed: () {
                SessionScope.of(context).stepUp();
                Navigator.of(context).pop(true);
              },
            ),
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const Text('Not now'),
            ),
          ],
        ),
      ),
    );
  }
}
