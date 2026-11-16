// Shared by the `subscriptions_rpc_cbor_seq*_test.dart` files
// (cratestack#1001): CBOR frames written by hand for the few shapes a frame
// has, a `CratestackHttpTransport` that answers with them, and a client wired
// to it. Copied beside the tests by `just verify-dart`.
import 'dart:convert';
import 'dart:typed_data';

import 'package:dart_verify_subscriptions_rpc/dart_verify_subscriptions_rpc.dart';

/// One CBOR item for the few shapes a frame has.
Uint8List seqCbor(Object? value) {
  final out = BytesBuilder();
  void head(int major, int n) {
    if (n < 24) {
      out.addByte(major << 5 | n);
    } else if (n < 256) {
      out.add(<int>[major << 5 | 24, n]);
    } else {
      out.add(<int>[major << 5 | 25, n >> 8, n & 0xff]);
    }
  }

  void write(Object? v) {
    switch (v) {
      case null:
        out.addByte(0xf6);
      case int():
        head(0, v);
      case String():
        final bytes = utf8.encode(v);
        head(3, bytes.length);
        out.add(bytes);
      case Map<String, Object?>():
        head(5, v.length);
        for (final entry in v.entries) {
          write(entry.key);
          write(entry.value);
        }
      default:
        throw ArgumentError('unsupported $v');
    }
  }

  write(value);
  return out.toBytes();
}

Uint8List seqJoin(List<Uint8List> parts) =>
    Uint8List.fromList(<int>[for (final part in parts) ...part]);

final seqKeepalive = Uint8List.fromList(<int>[0xf6]);
final seqReady = seqCbor(<String, Object?>{'id': 1, 'ready': <String, Object?>{}});
Uint8List seqNext(int id, String body) => seqCbor(<String, Object?>{
      'id': id,
      'next': <String, Object?>{'body': body},
    });
Uint8List seqEnd(int id) => seqCbor(<String, Object?>{'id': id, 'end': <String, Object?>{}});

class CborSeqTransport implements CratestackHttpTransport {
  CborSeqTransport(this.status, this.chunks, {this.contentType});

  final int status;
  final String? contentType;
  final List<Uint8List> chunks;
  final requests = <CratestackHttpRequest>[];

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    requests.add(request);
    return CratestackHttpResponse(
      status: status,
      headers: <String, String>{
        'content-type': contentType ?? (status == 200 ? 'application/cbor-seq' : 'application/json'),
      },
      body: Stream<Uint8List>.fromIterable(chunks),
    );
  }
}

DartVerifySubscriptionsRpcCratestackClient seqClient(CborSeqTransport transport, int cap) =>
    DartVerifySubscriptionsRpcCratestackClient(
      CratestackRpcHttpAdapter(transport, baseUri: Uri.parse('https://api.test')),
      subscriptions: CratestackSubscriptionOptions(
        transport: CratestackSubscriptionTransport.cborSeq,
        maxEventBytes: cap,
      ),
    );

Future<List<Line>> seqLines(
  CborSeqTransport transport, {
  int cap = cratestackDefaultMaxEventBytes,
  void Function()? onReady,
}) {
  return seqClient(transport, cap)
      .procedures
      .lines(const LinesArgs(args: Room(roomId: 'r1')), onReady: onReady)
      .toList();
}
