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
import 'package:cratestack_cbor/cratestack_cbor.dart';
import 'package:flutter/material.dart';

/// Prefix every marker line starts with, so verification can `grep` for a
/// single literal string regardless of OK/FAILED outcome.
const resultMarkerPrefix = 'CRATESTACK_CBOR_EXAMPLE_RESULT:';

void main() {
  runApp(const CratestackCborExampleApp());
}

class CratestackCborExampleApp extends StatelessWidget {
  const CratestackCborExampleApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'cratestack_cbor example',
      theme: ThemeData(
          colorScheme: ColorScheme.fromSeed(seedColor: Colors.deepPurple)),
      home: const RoundTripPage(),
    );
  }
}

class RoundTripPage extends StatefulWidget {
  const RoundTripPage({super.key});

  @override
  State<RoundTripPage> createState() => _RoundTripPageState();
}

class _RoundTripPageState extends State<RoundTripPage> {
  late final Future<RoundTripResult> _future = runRoundTrip();

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('cratestack_cbor round-trip')),
      body: Center(
        child: FutureBuilder<RoundTripResult>(
          future: _future,
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
