import 'models/widget.dart';
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

/// The adapter every generated API sends through. No default: the app picks
/// its HTTP stack by overriding this provider.
///
/// ```dart
/// ProviderScope(
///   overrides: [
///     tinyRpcClientAdapterProvider.overrideWithValue(
///       CratestackRpcHttpAdapter(
///         CratestackDioTransport(dio),
///         baseUri: Uri.parse('https://api.example.com'),
///       ),
///     ),
///   ],
///   child: const MyApp(),
/// )
/// ```
///
/// `CratestackDioTransport` wraps a `Dio`; any other HTTP stack implements
/// `CratestackHttpTransport` instead.
final tinyRpcClientAdapterProvider = Provider<CratestackRpcAdapter>((ref) {
  throw UnimplementedError('Override tinyRpcClientAdapterProvider before reading the generated CrateStack client.');
});

final tinyRpcClientClientProvider = Provider<TinyRpcClientCratestackClient>((ref) {
  return TinyRpcClientCratestackClient(ref.watch(tinyRpcClientAdapterProvider));
});
