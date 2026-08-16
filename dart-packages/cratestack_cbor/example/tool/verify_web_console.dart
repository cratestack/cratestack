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
// A command-line verification tool legitimately writes its result to
// stdout — that is its entire job — so `avoid_print` is disabled for the
// whole file rather than suppressed line by line.
// ignore_for_file: avoid_print
import 'dart:async';
import 'dart:convert';
import 'dart:io';

const _resultMarkerPrefix = 'CRATESTACK_CBOR_EXAMPLE_RESULT:';

Future<void> main(List<String> args) async {
  final options = _Options.parse(args);
  print('=== verify_web_console: launching headless Chrome ===');

  final chromeBinary =
      Platform.environment['CHROME_EXECUTABLE'] ?? 'google-chrome';
  final debugPort = options.debugPort;
  final process = await Process.start(chromeBinary, [
    '--headless=new',
    '--disable-gpu',
    '--no-sandbox',
    '--remote-debugging-port=$debugPort',
    '--remote-debugging-address=127.0.0.1',
    'about:blank',
  ]);
  process.stdout.transform(utf8.decoder).listen((_) {});
  process.stderr.transform(utf8.decoder).listen((_) {});

  try {
    await _waitForDevtoolsReady(debugPort);

    final targetInfo = await _openTarget(debugPort, options.url);
    final wsUrl = targetInfo['webSocketDebuggerUrl'] as String;
    final socket = await WebSocket.connect(wsUrl);

    final consoleLines = <String>[];
    final markerCompleter = Completer<String>();
    var nextId = 1;

    socket.listen((raw) {
      final message = jsonDecode(raw as String) as Map<String, dynamic>;
      if (message['method'] == 'Runtime.consoleAPICalled') {
        final params = message['params'] as Map<String, dynamic>;
        final args = params['args'] as List<dynamic>;
        final text = args
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
      socket.add(
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

    await socket.close();

    if (marker.isEmpty) {
      stderr.writeln(
        'FAIL: no "$_resultMarkerPrefix" console line observed within '
        '${options.timeoutSeconds}s. Captured console output:\n'
        '${consoleLines.map((l) => '  $l').join('\n')}',
      );
      exitCode = 1;
      return;
    }

    if (options.expectFailure) {
      if (marker.contains('FAILED')) {
        print('PASS (expected failure): $marker');
        exitCode = 0;
      } else {
        stderr.writeln('FAIL: expected a FAILED marker, got: $marker');
        exitCode = 1;
      }
      return;
    }

    if (marker.contains('OK')) {
      if (options.expectHex != null && !marker.contains(options.expectHex!)) {
        stderr.writeln(
          'FAIL: OK marker did not contain expected hex '
          '${options.expectHex}: $marker',
        );
        exitCode = 1;
        return;
      }
      print('PASS: $marker');
      exitCode = 0;
    } else {
      stderr.writeln('FAIL: expected an OK marker, got: $marker');
      exitCode = 1;
    }
  } finally {
    process.kill();
  }
}

Future<void> _waitForDevtoolsReady(int port) async {
  final client = HttpClient();
  final deadline = DateTime.now().add(const Duration(seconds: 15));
  while (DateTime.now().isBefore(deadline)) {
    try {
      final request = await client.getUrl(
        Uri.parse('http://127.0.0.1:$port/json/version'),
      );
      final response = await request.close();
      if (response.statusCode == 200) {
        await response.drain<void>();
        client.close();
        return;
      }
    } catch (_) {
      // Not up yet; retry.
    }
    await Future<void>.delayed(const Duration(milliseconds: 200));
  }
  client.close();
  throw StateError('Chrome DevTools Protocol never became ready on $port');
}

Future<Map<String, dynamic>> _openTarget(int port, String url) async {
  final client = HttpClient();
  // Recent Chrome versions require PUT (not GET) for /json/new — GET is
  // rejected with a plain-text CSRF warning, not JSON, which is what the
  // FormatException this comment replaced was actually diagnosing.
  final request = await client.putUrl(
    Uri.parse('http://127.0.0.1:$port/json/new?${Uri.encodeComponent(url)}'),
  );
  final response = await request.close();
  final body = await response.transform(utf8.decoder).join();
  client.close();
  return jsonDecode(body) as Map<String, dynamic>;
}

class _Options {
  _Options({
    required this.url,
    required this.timeoutSeconds,
    required this.debugPort,
    required this.expectFailure,
    this.expectHex,
  });

  final String url;
  final int timeoutSeconds;
  final int debugPort;
  final bool expectFailure;
  final String? expectHex;

  static _Options parse(List<String> args) {
    String? url;
    var timeoutSeconds = 15;
    var debugPort = 9333;
    var expectFailure = false;
    String? expectHex;
    for (var i = 0; i < args.length; i++) {
      switch (args[i]) {
        case '--url':
          url = args[++i];
        case '--timeout-seconds':
          timeoutSeconds = int.parse(args[++i]);
        case '--debug-port':
          debugPort = int.parse(args[++i]);
        case '--expect-failure':
          expectFailure = true;
        case '--expect-hex':
          expectHex = args[++i];
        default:
          throw ArgumentError('unknown argument: ${args[i]}');
      }
    }
    if (url == null) {
      throw ArgumentError('--url is required');
    }
    return _Options(
      url: url,
      timeoutSeconds: timeoutSeconds,
      debugPort: debugPort,
      expectFailure: expectFailure,
      expectHex: expectHex,
    );
  }
}
