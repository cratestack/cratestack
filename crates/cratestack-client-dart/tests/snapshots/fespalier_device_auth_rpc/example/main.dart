import 'package:device_auth_rpc_client/device_auth_rpc_client.dart';

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

  // Generated procedures:
  // - sendEnrolmentCode(...)
  // - beginEnrolment(...)
  // - confirmEnrolment(...)
  // - beginRenewal(...)
  // - confirmRenewal(...)
  // - revokeDevice(...)

}
