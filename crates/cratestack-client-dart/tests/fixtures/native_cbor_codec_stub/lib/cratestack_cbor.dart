/// Controllable stand-in for `package:cratestack_cbor`'s public surface —
/// see this package's `pubspec.yaml` for why a stub rather than the real
/// thing.
///
/// Mirrors only what a generated Dart client's `runtime.dart` actually
/// references: the `CratestackCborCodec` type, the `CratestackCborCodecError`
/// exception, and the async `createCborCodec()` factory. Everything else the
/// real package exports is irrelevant here and deliberately absent, so this
/// file fails to compile if the generated runtime ever starts depending on
/// more of that surface without this stub being updated to match.
library;

import 'dart:convert';
import 'dart:typed_data';

/// How many times [createCborCodec] has been invoked in this isolate.
///
/// The whole point of the test: with the memoization working, three
/// requests produce one call; with a failure in between, a *fourth* call
/// must produce a second invocation rather than replaying the cached
/// rejection forever.
int stubCborCodecCalls = 0;

/// When true, the next [createCborCodec] call throws instead of resolving,
/// then resets itself — a one-shot transient failure, which is the case
/// that distinguishes "cache successes only" from a plain `??=`.
bool stubCborCodecFailNext = false;

class CratestackCborCodecError implements Exception {
  const CratestackCborCodecError(this.message);

  final String message;

  @override
  String toString() => 'CratestackCborCodecError: $message';
}

abstract interface class CratestackCborCodec {
  String get contentType;

  Uint8List encodeJson(String json);

  String decodeJson(List<int> bytes);
}

Future<CratestackCborCodec> createCborCodec() async {
  stubCborCodecCalls++;
  // After an `await`, so the failure lands where a real one would: on the
  // returned future, not synchronously inside the caller's `??=`.
  await Future<void>.delayed(Duration.zero);
  if (stubCborCodecFailNext) {
    stubCborCodecFailNext = false;
    throw const CratestackCborCodecError('stub-induced codec failure');
  }
  return const _StubCodec();
}

/// Not real CBOR — the generated runtime only requires that whatever
/// `encodeJson` produces, `decodeJson` reverses. UTF-8 JSON text satisfies
/// that and keeps the stub inspectable.
class _StubCodec implements CratestackCborCodec {
  const _StubCodec();

  @override
  String get contentType => 'application/cbor';

  @override
  Uint8List encodeJson(String json) => Uint8List.fromList(utf8.encode(json));

  @override
  String decodeJson(List<int> bytes) => utf8.decode(bytes);
}
