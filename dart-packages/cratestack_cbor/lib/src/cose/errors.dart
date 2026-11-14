/// Why sealing a request or opening a response failed.
///
/// Sealed, so a `switch` over the caught exception is exhaustive.
/// [CoseRejected] is every failed verification check and carries no detail:
/// the client must not become an oracle for which check failed any more than
/// the server is (ADR 0006 §10). The other variants depend on local state
/// only, never on the received bytes, so they may say what went wrong.
sealed class CoseException implements Exception {
  const CoseException(this.message);

  /// What went wrong; empty for [CoseRejected], [CoseSignerCancelled] and
  /// [CoseSignerTimedOut].
  final String message;

  @override
  String toString() =>
      message.isEmpty ? '$runtimeType' : '$runtimeType: $message';
}

/// The message did not verify (the coarse `401`): a tampered body, a wrong
/// key, a response to another request, a stale or replayed message. Never
/// says which.
final class CoseRejected extends CoseException {
  const CoseRejected() : super('');
}

/// The caller or its configuration is wrong: a key of the wrong length, an
/// empty audience, a binding of the wrong shape.
final class CoseMisuse extends CoseException {
  const CoseMisuse(super.message);
}

/// The person dismissed the keystore's prompt: an [ExternalSigner] whose
/// callback returned [SignOutcome.cancelled].
final class CoseSignerCancelled extends CoseException {
  const CoseSignerCancelled() : super('');
}

/// The keystore did not answer in time: the callback of an [ExternalSigner]
/// returned [SignOutcome.timedOut], or exceeded [ExternalSigner.timeout].
final class CoseSignerTimedOut extends CoseException {
  const CoseSignerTimedOut() : super('');
}

/// The keystore failed: the callback of an [ExternalSigner] returned
/// [SignOutcome.failed] (the [message] is the callback's), or threw (the
/// message names only the exception's type). A signature that is not a valid
/// one by the key is [CoseMisuse] instead, because the callback did not fail.
final class CoseSignerFailed extends CoseException {
  const CoseSignerFailed(super.message);
}
