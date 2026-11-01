// The generated Dart client's WebTransport binding in a real browser (slice
// S6): compiled to JavaScript and run in headless Chromium against
// `cratestack-webtransport`'s `browser_harness` by `webtransport.cjs`. The
// harness's address and certificate hash come in as `window.__harness`;
// the outcome goes out as `window.__result`, one JSON document.
import 'dart:convert';
import 'dart:js_interop';
import 'dart:js_interop_unsafe';

// The runtime and the WebTransport module only: the package's library also
// exports its riverpod providers, which pull in Flutter.
import 'package:dart_verify_subscriptions_rpc/src/runtime.dart';
import 'package:dart_verify_subscriptions_rpc/src/subscription_webtransport_web.dart';

/// A fake HTTP transport: SSE must never be reached in these checks.
class _NoHttp implements CratestackHttpTransport {
  var used = false;

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) {
    used = true;
    throw StateError('SSE was used');
  }
}

Future<void> main() async {
  final harness = jsonDecode((globalContext['__harness'] as JSString).toDart) as Map<String, Object?>;
  final url = Uri.parse('https://127.0.0.1:${harness['port']}/rpc/wt');
  final hashes = <List<int>>[(harness['sha256'] as List).cast<int>()];
  final out = <String, Object?>{'supported': cratestackWebTransportSupported};
  Map<String, String> bearer(String token) => {'authorization': 'Bearer $token'};

  try {
    var ready = false;
    final items = await cratestackOpenWebTransport(url, hashes, bearer('7'), 'test.items', {'n': 3}, null)
        .then((stream) {
      ready = true;
      return stream.toList();
    });
    out['items'] = items;
    out['ready'] = ready;

    out['whoami'] = await (await cratestackOpenWebTransport(
            url, hashes, bearer('7'), 'test.whoami', null, null))
        .first;

    try {
      await cratestackOpenWebTransport(url, hashes, bearer('7'), 'test.refuse', null, null);
    } on CratestackSubscriptionException catch (error) {
      out['refused'] = error.code;
    }

    try {
      await cratestackOpenWebTransport(url, hashes, bearer('bad'), 'test.items', {'n': 1}, null);
    } on CratestackSubscriptionException catch (error) {
      out['badToken'] = error.code;
    }

    final forever = await cratestackOpenWebTransport(
        url, hashes, bearer('7'), 'test.forever', null, null);
    final subscription = forever.listen((_) {});
    await Future<void>.delayed(const Duration(milliseconds: 100));
    await subscription.cancel();
    out['cancelled'] = true;

    // `auto` with credentials and a listener: WebTransport, never SSE.
    final http = _NoHttp();
    final viaAuto = await cratestackSubscribe(
      options: CratestackSubscriptionOptions(
        webTransportUrl: url,
        serverCertificateHashes: hashes,
        credentials: () => bearer('7'),
      ),
      transport: http,
      baseUri: Uri.parse('http://localhost'),
      basePath: '',
      target: const CratestackSubscriptionTarget(
        opId: 'test.items',
        input: {'n': 2},
        ssePath: '/unused',
        sseQuery: null,
        socketPath: '/unused',
      ),
    ).toList();
    out['auto'] = viaAuto;
    out['autoUsedHttp'] = http.used;

    // A dead UDP port: `auto` gives up on WebTransport within its bound.
    final started = DateTime.now();
    try {
      await cratestackSubscribe(
        options: CratestackSubscriptionOptions(
          webTransportUrl: Uri.parse('https://127.0.0.1:${harness['deadPort']}/rpc/wt'),
          serverCertificateHashes: hashes,
          webSocketUrl: Uri.parse('ws://127.0.0.1:${harness['deadPort']}/rpc/ws'),
          credentials: () => bearer('7'),
        ),
        transport: _NoHttp(),
        baseUri: Uri.parse('http://localhost'),
        basePath: '',
        target: const CratestackSubscriptionTarget(
          opId: 'test.items',
          input: {'n': 1},
          ssePath: '/unused',
          sseQuery: null,
          socketPath: '/unused',
        ),
      ).toList();
    } catch (error) {
      out['fellBackToSse'] = '$error'.contains('SSE was used');
    }
    out['fallbackMillis'] = DateTime.now().difference(started).inMilliseconds;
  } catch (error, stackTrace) {
    out['error'] = '$error\n$stackTrace';
  }
  globalContext['__result'] = jsonEncode(out).toJS;
}
