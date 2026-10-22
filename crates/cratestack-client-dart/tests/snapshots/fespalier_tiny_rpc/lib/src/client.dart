import 'fespalier_transport.dart';
import 'models/widget.dart';
import 'package:fespalier_transport/fespalier_transport.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'procedures.dart';
import 'runtime.dart';

class TinyRpcClientCratestackClient {
  TinyRpcClientCratestackClient(this._adapter);

  final CratestackRpcAdapter _adapter;

  CratestackRpcAdapter get adapter => _adapter;

  WidgetApi get widgets => WidgetApi(this);
  ProceduresApi get procedures => ProceduresApi(this);
}

/// The transport every generated call sends through: your `fespalier_transport`
/// chain. No default; the app provides it once.
///
/// ```dart
/// ProviderScope(
///   overrides: [
///     tinyRpcClientTransportProvider.overrideWith(
///       (ref) => SessionTransport(
///         WriteGuardTransport(HttpClientTransport(client)),
///         ref.watch(credentialsProvider),
///       ),
///     ),
///     tinyRpcClientBaseUriProvider.overrideWithValue(
///       Uri.parse('https://api.example.com'),
///     ),
///   ],
///   child: const MyApp(),
/// )
/// ```
final tinyRpcClientTransportProvider = Provider<HttpTransport>((ref) {
  throw UnimplementedError('Override tinyRpcClientTransportProvider with your fespalier_transport chain before reading the generated CrateStack client.');
});

/// The server's origin, with an optional path prefix, that request paths are
/// joined onto: `https://api.example.com` or `https://example.com/v1`. No
/// default.
final tinyRpcClientBaseUriProvider = Provider<Uri>((ref) {
  throw UnimplementedError('Override tinyRpcClientBaseUriProvider with the server\'s origin before reading the generated CrateStack client.');
});

/// The adapter every generated API sends through: the HTTP adapter over
/// [tinyRpcClientTransportProvider]. Override it only to change the wire
/// format or to use another adapter.
final tinyRpcClientAdapterProvider = Provider<CratestackRpcAdapter>((ref) {
  return CratestackRpcHttpAdapter(
    CratestackFespalierTransport(ref.watch(tinyRpcClientTransportProvider)),
    baseUri: ref.watch(tinyRpcClientBaseUriProvider),
  );
});

final tinyRpcClientClientProvider = Provider<TinyRpcClientCratestackClient>((ref) {
  return TinyRpcClientCratestackClient(ref.watch(tinyRpcClientAdapterProvider));
});
