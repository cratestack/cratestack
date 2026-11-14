import 'dart:typed_data';

/// What a keystore signing call ended with.
///
/// [ExternalSigner]'s callback **returns** one of these instead of throwing,
/// so a dismissed prompt, a slow keystore and a broken one stay three
/// different things all the way to the caller (`CoseSignerCancelled`,
/// `CoseSignerTimedOut`, `CoseSignerFailed`). Sealed, so a `switch` over it is
/// exhaustive.
sealed class SignOutcome {
  const SignOutcome();

  /// The keystore signed. [signature] is the ECDSA signature over the bytes
  /// the callback was given, DER encoded or raw `r || s` (64 bytes); either
  /// spelling is accepted and normalised in Rust.
  factory SignOutcome.signature(Uint8List signature) = SignedOutcome;

  /// The person dismissed the keystore's prompt: [CoseSignerCancelled].
  const factory SignOutcome.cancelled() = CancelledOutcome;

  /// The keystore did not answer in time: [CoseSignerTimedOut]. A callback
  /// that gives up on its own reports this; [ExternalSigner.timeout] produces
  /// it for one that never answers.
  const factory SignOutcome.timedOut() = TimedOutOutcome;

  /// The keystore failed (key invalidated, hardware error, ...):
  /// [CoseSignerFailed] with [message]. Keep it free of secrets: it is shown
  /// in logs.
  const factory SignOutcome.failed(String message) = FailedOutcome;
}

/// The keystore signed; see [SignOutcome.signature].
final class SignedOutcome extends SignOutcome {
  /// Wraps a copy of [signature].
  SignedOutcome(Uint8List signature) : signature = Uint8List.fromList(signature);

  /// The signature (a copy).
  final Uint8List signature;
}

/// The prompt was dismissed; see [SignOutcome.cancelled].
final class CancelledOutcome extends SignOutcome {
  /// The prompt was dismissed.
  const CancelledOutcome();
}

/// The keystore was too slow; see [SignOutcome.timedOut].
final class TimedOutOutcome extends SignOutcome {
  /// The keystore was too slow.
  const TimedOutOutcome();
}

/// The keystore failed; see [SignOutcome.failed].
final class FailedOutcome extends SignOutcome {
  /// The keystore failed with [message].
  const FailedOutcome(this.message);

  /// What went wrong, as reported by the callback.
  final String message;
}

/// Signs the complete to-be-signed bytes with a keystore key: ECDSA over
/// SHA-256 of exactly [toBeSigned], never of a digest of it. Returns a
/// [SignOutcome]; see [ExternalSigner].
typedef SignCallback = Future<SignOutcome> Function(Uint8List toBeSigned);
