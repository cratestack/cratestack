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
) async {
  try {
    return await Future<SignOutcome>.sync(
      // A fresh copy: the callback must not be able to alter what the bridge
      // still holds.
      () => signer.sign(Uint8List.fromList(toBeSigned)),
    ).timeout(
      signer.timeout,
      onTimeout: () => const SignOutcome.timedOut(),
    );
  } catch (error) {
    return SignOutcome.failed(
      'the signing callback threw ${error.runtimeType}',
    );
  }
}
