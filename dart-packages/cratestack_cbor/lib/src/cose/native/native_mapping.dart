// The mappings between the package's types and the bridge's generated ones.
import 'dart:typed_data';

import '../../native/rust/cose/error.dart' as rust_error;
import '../../native/rust/cose/sign_outcome.dart' as rust_sign;
import '../../native/rust/cose/types.dart' as rust_types;
import '../errors.dart';
import '../sign_outcome.dart';
import '../types.dart';

rust_types.FlutterCallBinding toRustBinding(CallBinding binding) =>
    rust_types.FlutterCallBinding(
      method: binding.method,
      route: binding.route,
      pathParams: binding.pathParams,
      query: binding.query,
      contractSha: binding.contractSha,
      idempotencyKey: binding.idempotencyKey,
      ifMatch: binding.ifMatch,
      payloadType: binding.payloadType,
      payloadAccept: binding.payloadAccept,
    );

rust_sign.FlutterSignOutcome toRustOutcome(SignOutcome outcome) =>
    switch (outcome) {
      SignedOutcome() => rust_sign.FlutterSignOutcome(
          kind: rust_sign.FlutterSignOutcomeKind.signed,
          signature: outcome.signature,
          message: '',
        ),
      CancelledOutcome() => rust_sign.FlutterSignOutcome(
          kind: rust_sign.FlutterSignOutcomeKind.cancelled,
          signature: Uint8List(0),
          message: '',
        ),
      TimedOutOutcome() => rust_sign.FlutterSignOutcome(
          kind: rust_sign.FlutterSignOutcomeKind.timedOut,
          signature: Uint8List(0),
          message: '',
        ),
      FailedOutcome() => rust_sign.FlutterSignOutcome(
          kind: rust_sign.FlutterSignOutcomeKind.failed,
          signature: Uint8List(0),
          message: outcome.message,
        ),
    };

rust_types.FlutterCoseAlg toRustAlg(CoseAlg alg) => switch (alg) {
      CoseAlg.ed25519 => rust_types.FlutterCoseAlg.ed25519,
      CoseAlg.esp256 => rust_types.FlutterCoseAlg.esp256,
      CoseAlg.hmac256x64 => rust_types.FlutterCoseAlg.hmac25664,
      CoseAlg.hmac256x256 => rust_types.FlutterCoseAlg.hmac256256,
    };

CoseAlg fromRustAlg(rust_types.FlutterCoseAlg alg) => switch (alg) {
      rust_types.FlutterCoseAlg.ed25519 => CoseAlg.ed25519,
      rust_types.FlutterCoseAlg.esp256 => CoseAlg.esp256,
      rust_types.FlutterCoseAlg.hmac25664 => CoseAlg.hmac256x64,
      rust_types.FlutterCoseAlg.hmac256256 => CoseAlg.hmac256x256,
    };

CoseException coseException(rust_error.FlutterCoseError error) =>
    switch (error.kind) {
      rust_error.FlutterCoseErrorKind.rejected => const CoseRejected(),
      rust_error.FlutterCoseErrorKind.misuse => CoseMisuse(error.message),
      rust_error.FlutterCoseErrorKind.signerCancelled =>
        const CoseSignerCancelled(),
      rust_error.FlutterCoseErrorKind.signerTimedOut =>
        const CoseSignerTimedOut(),
      rust_error.FlutterCoseErrorKind.signerFailed =>
        CoseSignerFailed(error.message),
    };
