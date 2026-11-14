import 'fespalier_transport.dart';
import 'package:fespalier_transport/fespalier_transport.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'procedures.dart';
import 'runtime.dart';

class DeviceAuthRestClientCratestackClient {
  DeviceAuthRestClientCratestackClient(this._adapter, {this.basePath = '/api'});

  final CratestackClientAdapter _adapter;
  final String basePath;

  Future<Object?> execute(
    String method,
    String path, {
    Object? body,
    Map<String, Object?>? queryParameters,
    CratestackCallOptions? options,
  }) {
    return _adapter.execute(
      CratestackRequest(
        method: method,
        path: '$basePath$path',
        queryParameters: queryParameters,
        body: body,
      ),
      options: options,
    );
  }

  ProceduresApi get procedures => ProceduresApi(this);
}

/// The transport every generated call sends through: your `fespalier_transport`
/// chain. No default; the app provides it once.
///
/// ```dart
/// ProviderScope(
///   overrides: [
///     deviceAuthRestClientTransportProvider.overrideWith(
///       (ref) => SessionTransport(
///         WriteGuardTransport(HttpClientTransport(client)),
///         ref.watch(credentialsProvider),
///       ),
///     ),
///     deviceAuthRestClientBaseUriProvider.overrideWithValue(
///       Uri.parse('https://api.example.com'),
///     ),
///   ],
///   child: const MyApp(),
/// )
/// ```
final deviceAuthRestClientTransportProvider = Provider<HttpTransport>((ref) {
  throw UnimplementedError('Override deviceAuthRestClientTransportProvider with your fespalier_transport chain before reading the generated CrateStack client.');
});

/// The server's origin, with an optional path prefix, that request paths are
/// joined onto: `https://api.example.com` or `https://example.com/v1`. No
/// default.
final deviceAuthRestClientBaseUriProvider = Provider<Uri>((ref) {
  throw UnimplementedError('Override deviceAuthRestClientBaseUriProvider with the server\'s origin before reading the generated CrateStack client.');
});

/// Compact wire keys (opt-in): `null` keeps the wire named. Override it with
/// the hooks built from `cratestackKeyTables` to put CBOR calls on integer
/// keys:
///
/// ```dart
/// final keys = await createCompactKeys(cratestackKeyTables);
/// deviceAuthRestClientCompactProvider.overrideWithValue(
///   CratestackCompactHooks(
///     toCompact: keys.toCompact,
///     fromCompact: keys.fromCompact,
///     selector: keys.selector,
///   ),
/// )
/// ```
final deviceAuthRestClientCompactProvider = Provider<CratestackCompactHooks?>((ref) => null);

/// The adapter every generated API sends through: the HTTP adapter over
/// [deviceAuthRestClientTransportProvider]. Override it only to change the wire
/// format or to use another adapter.
final deviceAuthRestClientAdapterProvider = Provider<CratestackClientAdapter>((ref) {
  return CratestackHttpAdapter(
    CratestackFespalierTransport(ref.watch(deviceAuthRestClientTransportProvider)),
    baseUri: ref.watch(deviceAuthRestClientBaseUriProvider),
    compact: ref.watch(deviceAuthRestClientCompactProvider),
  );
});

final deviceAuthRestClientBasePathProvider = Provider<String>((ref) => '/api');

final deviceAuthRestClientClientProvider = Provider<DeviceAuthRestClientCratestackClient>((ref) {
  return DeviceAuthRestClientCratestackClient(
    ref.watch(deviceAuthRestClientAdapterProvider),
    basePath: ref.watch(deviceAuthRestClientBasePathProvider),
  );
});
