// Headless-Chrome verification for a RELEASE `flutter build web` bundle
// (cratestack#563's "Flutter Web asset bundling" slice).
//
// The decisive gap this closes: `dart test -p chrome` and `flutter run -d
// chrome` both serve this package through a dev server with a special
// `packages/...` URL route that a release bundle does not have (see
// `lib/src/web/web_cbor_codec.dart`'s doc comment). Proving the web backend
// therefore requires actually SERVING `build/web/` — the real release
// output — and loading it in a real browser, not the dev server.
//
// Flutter web renders through CanvasKit/skwasm onto a single <canvas>, so
// there is no meaningful DOM text to scrape (`document.body.innerText`
// would find nothing). Instead this drives the DevTools Protocol directly
// (`dart:io`'s `WebSocket`, no extra package needed) and listens for
// `Runtime.consoleAPICalled` — the example app's `print()` calls
// (`lib/main.dart`'s `CRATESTACK_CBOR_EXAMPLE_RESULT:` marker) reach the
// browser console exactly the way any Dart `print()` does under dart2js.
//
// Usage:
//   dart run tool/verify_web_console.dart --url http://127.0.0.1:PORT/
//
// Exits 0 and prints the captured marker line if `... OK <hex>` appears
// within the timeout; exits 1 otherwise (including if `... FAILED ...`
// appears, or nothing appears at all) — see this package's README for how
// this is used both for the happy path and the "corrupt the artifact"
// negative proof.
//
// Chrome launching and DevTools-readiness waiting (including the one
// automatic relaunch on a readiness failure) live in
// `verify_web_console/devtools_ready.dart` — split out to keep this file
// under the repo's 200-line-per-file convention and because that logic is
// independently unit-testable without a real Chrome (see
// `verify_web_console/self_test.dart`).
//
// Every exit path below tears down deterministically (cancel the DevTools
// socket subscription, close the socket, `ChromeProcess.shutDown` — see
// that method's doc) and finishes with an explicit `exit(code)` rather
// than returning and trusting the event loop to drain on its own — see
// `verify_web_console/hard_timeout_watchdog.dart`'s module doc for why
// that trust broke once already (PR 887's own CI run hung 45 minutes).
// The watchdog started at the top of `main` is the backstop if it ever
// breaks again.
//
// A command-line verification tool legitimately writes its result to
// stdout — that is its entire job — so `avoid_print` is disabled for the
// whole file rather than suppressed line by line.
// ignore_for_file: avoid_print
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'verify_web_console/chrome_launch.dart';
import 'verify_web_console/devtools_ready.dart';
import 'verify_web_console/hard_timeout_watchdog.dart';
import 'verify_web_console/options.dart';

const _resultMarkerPrefix = 'CRATESTACK_CBOR_EXAMPLE_RESULT:';

Future<void> main(List<String> args) async {
  final options = Options.parse(args);
  final watchdog = HardTimeoutWatchdog(
    Duration(seconds: options.hardTimeoutSeconds),
  );
  print('=== verify_web_console: launching headless Chrome ===');

  var code = 1;
  ChromeProcess? chrome;
  WebSocket? socket;
  StreamSubscription<dynamic>? socketSub;

  try {
    chrome = await ensureChromeReady(
      initialPort: options.debugPort,
      timeout: Duration(seconds: options.devtoolsReadyTimeoutSeconds),
    );

    final targetInfo = await _openTarget(chrome.port, options.url);
    final wsUrl = targetInfo['webSocketDebuggerUrl'] as String;
    socket = await WebSocket.connect(wsUrl);

    final consoleLines = <String>[];
    final markerCompleter = Completer<String>();
    var nextId = 1;

    socketSub = socket.listen((raw) {
      final message = jsonDecode(raw as String) as Map<String, dynamic>;
      if (message['method'] == 'Runtime.consoleAPICalled') {
        final params = message['params'] as Map<String, dynamic>;
        final consoleArgs = params['args'] as List<dynamic>;
        final text = consoleArgs
            .map((a) => (a as Map<String, dynamic>)['value']?.toString() ?? '')
            .join(' ');
        consoleLines.add(text);
        print('[console] $text');
        if (text.contains(_resultMarkerPrefix) &&
            !markerCompleter.isCompleted) {
          markerCompleter.complete(text);
        }
      }
    });

    void send(String method, [Map<String, dynamic>? params]) {
      socket!.add(
        jsonEncode({'id': nextId++, 'method': method, 'params': params ?? {}}),
      );
    }

    send('Runtime.enable');
    send('Page.enable');
    send('Page.navigate', {'url': options.url});

    final marker = await markerCompleter.future.timeout(
      Duration(seconds: options.timeoutSeconds),
      onTimeout: () => '',
    );

    code = _evaluateMarker(marker, options, consoleLines);
  } catch (e, stackTrace) {
    stderr.writeln(
        'FAIL: unexpected error in verify_web_console: $e\n$stackTrace');
    code = 1;
  } finally {
    await socketSub?.cancel();
    if (socket != null) {
      await socket.close().catchError((Object _) {});
    }
    if (chrome != null) {
      await chrome.shutDown();
    }
    watchdog.cancel();
  }
  exit(code);
}

/// Pure evaluation of the captured marker against [options] — extracted
/// so `main`'s teardown (`finally`, `exit(code)`) has exactly one call
/// site to reason about instead of the four early-return branches the
/// inline version used to have.
int _evaluateMarker(
  String marker,
  Options options,
  List<String> consoleLines,
) {
  if (marker.isEmpty) {
    stderr.writeln(
      'FAIL: no "$_resultMarkerPrefix" console line observed within '
      '${options.timeoutSeconds}s. Captured console output:\n'
      '${consoleLines.map((l) => '  $l').join('\n')}',
    );
    return 1;
  }

  if (options.expectFailure) {
    if (marker.contains('FAILED')) {
      print('PASS (expected failure): $marker');
      return 0;
    }
    stderr.writeln('FAIL: expected a FAILED marker, got: $marker');
    return 1;
  }

  if (!marker.contains('OK')) {
    stderr.writeln('FAIL: expected an OK marker, got: $marker');
    return 1;
  }
  if (options.expectHex != null && !marker.contains(options.expectHex!)) {
    stderr.writeln(
      'FAIL: OK marker did not contain expected hex '
      '${options.expectHex}: $marker',
    );
    return 1;
  }
  print('PASS: $marker');
  return 0;
}

Future<Map<String, dynamic>> _openTarget(int port, String url) async {
  final client = HttpClient();
  try {
    // Recent Chrome versions require PUT (not GET) for /json/new — GET is
    // rejected with a plain-text CSRF warning, not JSON, which is what the
    // FormatException this comment replaced was actually diagnosing.
    final request = await client.putUrl(
      Uri.parse('http://127.0.0.1:$port/json/new?${Uri.encodeComponent(url)}'),
    );
    final response = await request.close();
    final body = await response.transform(utf8.decoder).join();
    return jsonDecode(body) as Map<String, dynamic>;
  } finally {
    client.close(force: true);
  }
}
