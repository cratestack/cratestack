import 'fespalier_transport.dart';
import 'models/widget.dart';
import 'package:fespalier_transport/fespalier_transport.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'procedures.dart';
import 'runtime.dart';

class TinyRestClientCratestackClient {
  TinyRestClientCratestackClient(this._adapter, {this.basePath = '/api'});

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

  WidgetApi get widgets => WidgetApi(this);
  ProceduresApi get procedures => ProceduresApi(this);
}

/// The transport every generated call sends through: your `fespalier_transport`
/// chain. No default; the app provides it once.
///
/// ```dart
/// ProviderScope(
///   overrides: [
///     tinyRestClientTransportProvider.overrideWith(
///       (ref) => SessionTransport(
///         WriteGuardTransport(HttpClientTransport(client)),
///         ref.watch(credentialsProvider),
///       ),
///     ),
///     tinyRestClientBaseUriProvider.overrideWithValue(
///       Uri.parse('https://api.example.com'),
///     ),
///   ],
///   child: const MyApp(),
/// )
/// ```
final tinyRestClientTransportProvider = Provider<HttpTransport>((ref) {
  throw UnimplementedError('Override tinyRestClientTransportProvider with your fespalier_transport chain before reading the generated CrateStack client.');
});

/// The server's origin, with an optional path prefix, that request paths are
/// joined onto: `https://api.example.com` or `https://example.com/v1`. No
/// default.
final tinyRestClientBaseUriProvider = Provider<Uri>((ref) {
  throw UnimplementedError('Override tinyRestClientBaseUriProvider with the server\'s origin before reading the generated CrateStack client.');
});

/// The adapter every generated API sends through: the HTTP adapter over
/// [tinyRestClientTransportProvider]. Override it only to change the wire
/// format or to use another adapter.
final tinyRestClientAdapterProvider = Provider<CratestackClientAdapter>((ref) {
  return CratestackHttpAdapter(
    CratestackFespalierTransport(ref.watch(tinyRestClientTransportProvider)),
    baseUri: ref.watch(tinyRestClientBaseUriProvider),
  );
});

final tinyRestClientBasePathProvider = Provider<String>((ref) => '/api');

final tinyRestClientClientProvider = Provider<TinyRestClientCratestackClient>((ref) {
  return TinyRestClientCratestackClient(
    ref.watch(tinyRestClientAdapterProvider),
    basePath: ref.watch(tinyRestClientBasePathProvider),
  );
});
