import 'package:flutter/material.dart';
import 'package:flutter_riverpod_client/flutter_riverpod_client.dart';

/// The refusal's code: the `error` member an `AuthRefusal` adds (RFC 9470's
/// `insufficient_user_authentication` for a step-up), else the envelope's
/// `code` (`FORBIDDEN`, `UNAUTHORIZED`, ...). `null` for an error that is not
/// a server answer.
String? refusalCode(Object error) {
  if (error is! CratestackRestException) {
    return null;
  }
  final member = error.extra['error'];
  return member is String ? member : error.code;
}

/// What the banner says for each code.
String refusalText(String? code) => switch (code) {
      cratestackInsufficientUserAuthentication =>
        'This needs a fresh sign-in with your security key.',
      'FORBIDDEN' => 'You are not allowed to do this.',
      'UNAUTHORIZED' => 'Your session has ended. Sign in again.',
      null => 'Something went wrong. Try again.',
      _ => 'The server refused this ($code).',
    };

/// A banner keyed on the refusal code, with the code shown so a support
/// request can name it. [onRetry] offers the step-up again.
class RefusalBanner extends StatelessWidget {
  const RefusalBanner({super.key, required this.error, this.onRetry});

  final Object error;
  final VoidCallback? onRetry;

  @override
  Widget build(BuildContext context) {
    final code = refusalCode(error);
    return MaterialBanner(
      key: ValueKey('refusal-${code ?? 'unknown'}'),
      leading: Icon(
        code == cratestackInsufficientUserAuthentication
            ? Icons.lock_clock
            : Icons.error_outline,
      ),
      content: Text('${refusalText(code)}${code == null ? '' : '\n$code'}'),
      actions: [
        if (onRetry != null && code == cratestackInsufficientUserAuthentication)
          TextButton(onPressed: onRetry, child: const Text('Confirm now'))
        else
          const SizedBox.shrink(),
      ],
    );
  }
}
