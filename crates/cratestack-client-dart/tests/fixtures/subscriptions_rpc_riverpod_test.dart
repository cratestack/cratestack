// Slice S6 in `--preset riverpod`: the generated stream providers, read
// through a `ProviderContainer` whose adapter answers with an event stream.
// A subscription procedure's provider emits its items; a model's events
// provider decodes `CratestackModelEvent`s and is one provider per distinct
// input, compared by value.
import 'dart:convert';
import 'dart:typed_data';

import 'package:dart_verify_riverpod_subscriptions_rpc/dart_verify_riverpod_subscriptions_rpc.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

class _SseTransport implements CratestackHttpTransport {
  _SseTransport(this.chunks);

  final List<String> chunks;
  final requests = <CratestackHttpRequest>[];

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    requests.add(request);
    return CratestackHttpResponse(
      status: 200,
      headers: const <String, String>{'content-type': 'text/event-stream'},
      body: Stream<Uint8List>.fromIterable(
        chunks.map((chunk) => Uint8List.fromList(utf8.encode(chunk))),
      ),
    );
  }
}

ProviderContainer _container(_SseTransport transport) {
  final container = ProviderContainer(
    overrides: [
      dartVerifyRiverpodSubscriptionsRpcAdapterProvider.overrideWithValue(
        CratestackRpcHttpAdapter(transport, baseUri: Uri.parse('https://api.test')),
      ),
    ],
  );
  addTearDown(container.dispose);
  return container;
}

void main() {
  test('a subscription procedure provider emits its items', () async {
    final transport = _SseTransport(<String>[
      'event: ready\ndata: {"id":1,"ready":{}}\n\n',
      'event: message\ndata: {"id":2,"next":{"body":"hi"}}\n\n',
    ]);
    final container = _container(transport);
    final subscription = container.listen(
      linesProvider(const LinesArgs(args: Room(roomId: 'r1'))).future,
      (_, __) {},
    );
    final line = await subscription.read();
    expect(line.body, 'hi');
    expect(
      transport.requests.single.uri.path,
      '/rpc/subscribe/procedure.lines',
    );
  });

  test('a model events provider decodes events, one per distinct input', () async {
    final transport = _SseTransport(<String>[
      'event: ready\ndata: {"id":1,"ready":{}}\n\n',
      'event: message\ndata: {"id":2,"next":{"event_id":"e1","model":"Note",'
          '"operation":"Created","occurred_at":"2026-11-01T00:00:00Z",'
          '"data":{"id":7,"body":"mine"}}}\n\n',
    ]);
    final container = _container(transport);
    final input = CratestackSubscribeInput(
      filters: <String, String>{'body__contains': 'mi'},
    );
    expect(
      noteEventsProvider(input: input),
      noteEventsProvider(
        input: const CratestackSubscribeInput(
          filters: <String, String>{'body__contains': 'mi'},
        ),
      ),
    );
    final subscription = container.listen(
      noteEventsProvider(input: input).future,
      (_, __) {},
    );
    final event = await subscription.read();
    expect(event.operation, 'Created');
    expect(event.data.body, 'mine');
    expect(transport.requests.single.uri.query, 'body__contains=mi');
  });
}
