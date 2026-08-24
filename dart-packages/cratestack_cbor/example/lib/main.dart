// Minimal example app exercising `package:cratestack_cbor` end to end
// (cratestack#563's "Flutter app integration" slice). This is deliberately
// the whole point of this directory: prove the package works inside a REAL
// `flutter build linux` / `flutter build web` — not `dart test` — by
// actually calling `createCborCodec()` and round-tripping a value through
// it at app startup.
//
// The result is both shown on screen AND printed to stdout with a unique,
// greppable marker. The stdout print is load-bearing, not decorative: a
// built desktop binary or a served release web bundle has no interactive
// console to casually read, so headless verification (`just
// cbor-example-verify-linux`, and the equivalent headless-Chrome check for
// web) greps process/console output for this marker rather than trying to
// screenshot a GUI. See this package's README for the verification
// transcripts, including the "corrupt the vendored artifact -> the app
// fails, loudly" proof.
import 'dart:async';

import 'package:cratestack_cbor/cratestack_cbor.dart';
import 'package:flutter/material.dart';

/// Prefix every marker line starts with, so verification can `grep` for a
/// single literal string regardless of OK/FAILED outcome.
const resultMarkerPrefix = 'CRATESTACK_CBOR_EXAMPLE_RESULT:';

// THE ROUND TRIP STARTS HERE, NOT IN A WIDGET.
//
// It used to hang off `late final _future = runRoundTrip()` on the page's
// State, whose only read was inside `build()`. `late final` initializes
// lazily, so the marker `print` that every headless verification greps for
// was a side effect of building a widget — the app had to construct its
// tree successfully before the assertion everything downstream depends on
// could even be attempted.
//
// HOW MUCH LATER, precisely, because an earlier version of this comment got
// it wrong and claimed the round trip waited for a rendered frame. It did
// not. `runApp` calls `scheduleAttachRootWidget`, which is a bare
// `Timer.run(() => attachRootWidget(...))`, and `attachToBuildOwner`
// inflates the element tree synchronously inside `buildScope` — so `build()`
// ran one event-loop turn after `runApp`, with no frame and no platform
// scene required. See `packages/flutter/lib/src/widgets/binding.dart`.
//
// So this change buys one thing, not two: the marker no longer depends on
// the widget tree building at all. It does NOT rescue a platform that never
// starts the Dart isolate — on iOS the engine is launched from
// `FlutterViewController.viewDidLoad`, upstream of any Dart code, in both
// shapes. cratestack#704 remains open and unexplained; do not read this
// comment as its fix.
void main() {
  // The round trip runs before `runApp`, so the binding is not yet
  // initialized as a side effect of it. Neither backend needs a platform
  // channel today (native resolves a path via `dart:io`/`dart:isolate`,
  // web injects a script tag), but doing pre-`runApp` async work without
  // this is the documented way to get bitten the moment one of them does.
  WidgetsFlutterBinding.ensureInitialized();
  final roundTrip = runRoundTrip();
  // Attach a listener NOW. `runRoundTrip` rethrows after printing the
  // FAILED marker, and between here and the first `build()` the
  // `FutureBuilder` below is not listening yet — an error landing in that
  // window would be reported as an unhandled async error on top of the
  // marker we actually want read. The FutureBuilder still surfaces it.
  unawaited(
      roundTrip.then<void>((_) {}, onError: (Object _, StackTrace __) {}));
  runApp(CratestackCborExampleApp(roundTrip: roundTrip));
}

class CratestackCborExampleApp extends StatelessWidget {
  const CratestackCborExampleApp({super.key, required this.roundTrip});

  /// The round trip started by [main], already in flight. Passed in rather
  /// than started on demand — see [main]'s comment for why that matters.
  final Future<RoundTripResult> roundTrip;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'cratestack_cbor example',
      theme: ThemeData(
          colorScheme: ColorScheme.fromSeed(seedColor: Colors.deepPurple)),
      home: RoundTripPage(roundTrip: roundTrip),
    );
  }
}

class RoundTripPage extends StatelessWidget {
  const RoundTripPage({super.key, required this.roundTrip});

  /// See [CratestackCborExampleApp.roundTrip].
  final Future<RoundTripResult> roundTrip;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('cratestack_cbor round-trip')),
      body: Center(
        child: FutureBuilder<RoundTripResult>(
          future: roundTrip,
          builder: (context, snapshot) {
            if (snapshot.connectionState != ConnectionState.done) {
              return const CircularProgressIndicator();
            }
            if (snapshot.hasError) {
              return Padding(
                padding: const EdgeInsets.all(24),
                child: Text(
                  key: const Key('cratestack_cbor_result'),
                  'ROUND-TRIP FAILED\n${snapshot.error}',
                  textAlign: TextAlign.center,
                  style: const TextStyle(color: Colors.red),
                ),
              );
            }
            final result = snapshot.data!;
            return Padding(
              padding: const EdgeInsets.all(24),
              child: Text(
                key: const Key('cratestack_cbor_result'),
                'ROUND-TRIP OK\n'
                'backend content-type: ${result.contentType}\n'
                'input:    ${result.input}\n'
                'cbor hex: ${result.hex}\n'
                'output:   ${result.output}',
                textAlign: TextAlign.center,
              ),
            );
          },
        ),
      ),
    );
  }
}

class RoundTripResult {
  const RoundTripResult({
    required this.contentType,
    required this.input,
    required this.hex,
    required this.output,
  });

  final String contentType;
  final String input;
  final String hex;
  final String output;
}

/// Calls the real, public `createCborCodec()`/`CratestackCborCodec` API
/// (not a `src/`-internal shortcut) and round-trips a fixed JSON value
/// through it, matching one of the package's own shared cross-binding
/// fixtures (`../test/shared_fixtures.dart`) so the printed hex can be
/// checked against a known-good value, not just "didn't throw".
Future<RoundTripResult> runRoundTrip() async {
  const input = '{"cratestack":["cool","stack"],"n":42,"ok":true}';
  try {
    final codec = await createCborCodec();
    final bytes = codec.encodeJson(input);
    final hex = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
    final output = codec.decodeJson(bytes);
    final result = RoundTripResult(
      contentType: codec.contentType,
      input: input,
      hex: hex,
      output: output,
    );
    // ignore: avoid_print
    print('$resultMarkerPrefix OK $hex');
    return result;
  } catch (error, stackTrace) {
    // ignore: avoid_print
    print('$resultMarkerPrefix FAILED $error');
    Error.throwWithStackTrace(error, stackTrace);
  }
}
