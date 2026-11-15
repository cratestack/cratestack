import 'dart:async';
import 'dart:typed_data';

import 'sign_outcome.dart';
import 'signers.dart';

/// Calls [signer]'s callback for one seal and always returns an outcome.
///
/// Both backends route through this, so the contract is one piece of code:
///
/// - [ExternalSigner.timeout] bounds the call; exceeding it is
///   [TimedOutOutcome]. The callback is not interrupted (Dart cannot), only
///   its answer is no longer waited for.
/// - A throw from the callback, synchronous or not, is [FailedOutcome] naming
///   only the exception's type: the text of an error out of a keystore plugin
///   can carry anything, and a bridge future that completes with an error
///   would panic the Rust side.
Future<SignOutcome> invokeSigner(
  ExternalSigner signer,
  Uint8List toBeSigned,
) {
  final done = Completer<SignOutcome>();
  late final Timer timer;
  // First end wins: later ones (a callback answering after its timeout, or
  // throwing after it answered) are dropped, never rethrown.
  void finish(SignOutcome outcome) {
    if (done.isCompleted) return;
    timer.cancel();
    done.complete(outcome);
  }

  SignOutcome failure(Object error) => SignOutcome.failed(
        'the signing callback threw ${error.runtimeType}',
      );

  timer = Timer(signer.timeout, () => finish(const SignOutcome.timedOut()));
  // The callback runs in its own error zone. The bridge calls this function
  // from the zone it was set up in, which can belong to someone else (the
  // first caller, a test), while the callback's closure belongs to whoever
  // built the signer: left alone, a throw out of `async` code is reported as
  // uncaught in a zone that does not own the future. A zone of our own
  // keeps the callback's futures and their errors together, and turns an
  // error with no listener (a throw after the answer, from a timer the
  // callback started) into a value or into nothing.
  runZonedGuarded(
    () => Future<SignOutcome>.sync(
      // A fresh copy: the callback must not be able to alter what the bridge
      // still holds.
      () => signer.sign(Uint8List.fromList(toBeSigned)),
    ).then(
      finish,
      onError: (Object error, StackTrace _) => finish(failure(error)),
    ),
    (error, _) => finish(failure(error)),
  );
  return done.future;
}
