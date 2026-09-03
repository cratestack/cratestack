// A minimal fake of Chrome's DevTools Protocol HTTP+WebSocket surface,
// used only by `self_test.dart` to drive the REAL `verify_web_console.dart`
// entry point as a subprocess without a real browser. Implements exactly
// the three calls `verify_web_console.dart` makes: `GET /json/version`
// (readiness), `PUT /json/new?url` (open a target), and the target's
// WebSocket, on which it optionally emits a single
// `Runtime.consoleAPICalled` event carrying the marker text.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

class FakeDevToolsServer {
  FakeDevToolsServer._(this._httpServer);

  final HttpServer _httpServer;

  /// Starts listening on [port]. If [markerText] is non-null, every
  /// WebSocket connection is sent one `Runtime.consoleAPICalled` event
  /// carrying it shortly after connecting; if null, connections are
  /// accepted but stay silent forever — simulating "the marker never
  /// arrives" for the watchdog self-test.
  static Future<FakeDevToolsServer> start(
    int port, {
    String? markerText,
  }) async {
    final httpServer =
        await HttpServer.bind(InternetAddress.loopbackIPv4, port);
    final fake = FakeDevToolsServer._(httpServer);
    httpServer.listen((request) => fake._handle(request, markerText));
    return fake;
  }

  Future<void> _handle(HttpRequest request, String? markerText) async {
    if (request.uri.path == '/json/version') {
      await _respondJson(request, {'Browser': 'FakeChrome/self-test'});
      return;
    }
    if (request.uri.path == '/json/new') {
      await _respondJson(request, {
        'id': 'fake-target',
        'webSocketDebuggerUrl':
            'ws://127.0.0.1:${_httpServer.port}/devtools/page/fake-target',
      });
      return;
    }
    if (WebSocketTransformer.isUpgradeRequest(request)) {
      final socket = await WebSocketTransformer.upgrade(request);
      if (markerText != null) {
        unawaited(_emitMarkerShortly(socket, markerText));
      }
      // Drain (and ignore) whatever the client sends — Runtime.enable /
      // Page.enable / Page.navigate — same as a real target would accept
      // commands without this fake needing to understand them.
      socket.listen((_) {});
      return;
    }
    request.response.statusCode = HttpStatus.notFound;
    await request.response.close();
  }

  Future<void> _emitMarkerShortly(WebSocket socket, String markerText) async {
    await Future<void>.delayed(const Duration(milliseconds: 200));
    socket.add(
      jsonEncode({
        'method': 'Runtime.consoleAPICalled',
        'params': {
          'args': [
            {'value': markerText},
          ],
        },
      }),
    );
  }

  Future<void> _respondJson(HttpRequest request, Object body) async {
    request.response
      ..statusCode = HttpStatus.ok
      ..headers.contentType = ContentType.json
      ..write(jsonEncode(body));
    await request.response.close();
  }

  Future<void> close() => _httpServer.close(force: true);
}
