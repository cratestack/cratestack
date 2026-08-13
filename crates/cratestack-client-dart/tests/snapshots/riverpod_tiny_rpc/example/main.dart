import 'package:tiny_rpc_client/tiny_rpc_client.dart';

void main() {
  // `CratestackRpcCallOptions` (headers/idempotency key) is the RPC
  // transport's per-call options type — the REST transport's URL-query
  // builder types have no equivalent here. RPC calls carry a typed body
  // instead of a URL query, so there is no query-builder surface to
  // exercise in this package.
  const options = CratestackRpcCallOptions(idempotencyKey: 'example-key');
  assert(options.idempotencyKey == 'example-key');
  assert(options.headers.isEmpty);

  // Generated model API entry points:
  // - widgets

  // Generated procedures:
  // - echoName(...)

  // Round-trips the generated model class through the same
  // fromWire/toWire pair every RPC response and request body uses.
  final sample = Widget.fromWire(const <String, Object?>{});
  final wire = sample.toWire();
  assert(wire.containsKey('id'));
}
