import 'package:flutter_test/flutter_test.dart';
import 'package:device_auth_rpc_client/device_auth_rpc_client.dart';

void main() {
  // A real `test()` case, not bare top-level `assert`s — the latter are
  // no-ops in a release-mode `flutter test` run, and a `test/` file with
  // no `test()` case at all when `override_proof` is unset (schemas with
  // no models, or whose first model in schema order is paged) is itself
  // a smell for a generated test scaffold. Wrapping this in `test(...)`
  // also gives `flutter_test` a real, unconditional use, independent of
  // whether the `ProviderContainer` tests below are emitted.
  test('rpc call options and generated surface', () {
    // `CratestackRpcCallOptions` (headers/idempotency key) is the RPC
    // transport's per-call options type — the REST transport's URL-query
    // builder types have no equivalent here. RPC calls carry a typed body
    // instead of a URL query, so there is no query-builder surface to
    // exercise in this package.
    const options = CratestackRpcCallOptions(
      headers: {'x-client': 'example'},
      idempotencyKey: 'example-key',
    );
    expect(options.idempotencyKey, 'example-key');
    expect(options.headers['x-client'], 'example');

    // Generated model API entry points:

    // Generated procedures:
    // - sendEnrolmentCode(...)
    // - beginEnrolment(...)
    // - confirmEnrolment(...)
    // - beginRenewal(...)
    // - confirmRenewal(...)
    // - revokeDevice(...)

  });

}
