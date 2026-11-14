import 'fespalier_transport.dart';
import 'package:fespalier_transport/fespalier_transport.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'procedures.dart';
import 'runtime.dart';

class DeviceAuthRpcClientCratestackClient {
  DeviceAuthRpcClientCratestackClient(this._adapter);

  final CratestackRpcAdapter _adapter;

  CratestackRpcAdapter get adapter => _adapter;

  ProceduresApi get procedures => ProceduresApi(this);
}

/// The transport every generated call sends through: your `fespalier_transport`
/// chain. No default; the app provides it once.
///
/// ```dart
/// ProviderScope(
///   overrides: [
///     deviceAuthRpcClientTransportProvider.overrideWith(
///       (ref) => SessionTransport(
///         WriteGuardTransport(HttpClientTransport(client)),
///         ref.watch(credentialsProvider),
///       ),
///     ),
///     deviceAuthRpcClientBaseUriProvider.overrideWithValue(
///       Uri.parse('https://api.example.com'),
///     ),
///   ],
///   child: const MyApp(),
/// )
/// ```
final deviceAuthRpcClientTransportProvider = Provider<HttpTransport>((ref) {
  throw UnimplementedError('Override deviceAuthRpcClientTransportProvider with your fespalier_transport chain before reading the generated CrateStack client.');
});

/// The server's origin, with an optional path prefix, that request paths are
/// joined onto: `https://api.example.com` or `https://example.com/v1`. No
/// default.
final deviceAuthRpcClientBaseUriProvider = Provider<Uri>((ref) {
  throw UnimplementedError('Override deviceAuthRpcClientBaseUriProvider with the server\'s origin before reading the generated CrateStack client.');
});

/// Compact wire keys (opt-in): `null` keeps the wire named. Override it with
/// the hooks built from `cratestackKeyTables` to put CBOR calls on integer
/// keys:
///
/// ```dart
/// final keys = await createCompactKeys(cratestackKeyTables);
/// deviceAuthRpcClientCompactProvider.overrideWithValue(
///   CratestackCompactHooks(
///     toCompact: keys.toCompact,
///     fromCompact: keys.fromCompact,
///     selector: keys.selector,
///   ),
/// )
/// ```
final deviceAuthRpcClientCompactProvider = Provider<CratestackCompactHooks?>((ref) => null);

/// The adapter every generated API sends through: the HTTP adapter over
/// [deviceAuthRpcClientTransportProvider]. Override it only to change the wire
/// format or to use another adapter.
final deviceAuthRpcClientAdapterProvider = Provider<CratestackRpcAdapter>((ref) {
  return CratestackRpcHttpAdapter(
    CratestackFespalierTransport(ref.watch(deviceAuthRpcClientTransportProvider)),
    baseUri: ref.watch(deviceAuthRpcClientBaseUriProvider),
    compact: ref.watch(deviceAuthRpcClientCompactProvider),
  );
});

final deviceAuthRpcClientClientProvider = Provider<DeviceAuthRpcClientCratestackClient>((ref) {
  return DeviceAuthRpcClientCratestackClient(ref.watch(deviceAuthRpcClientAdapterProvider));
});
