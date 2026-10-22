import 'models/board.dart';
import 'models/task.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'procedures.dart';
import 'runtime.dart';

class FlutterRiverpodClientCratestackClient {
  FlutterRiverpodClientCratestackClient(this._adapter, {this.basePath = '/api'});

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

  BoardApi get boards => BoardApi(this);
  TaskApi get tasks => TaskApi(this);
  ProceduresApi get procedures => ProceduresApi(this);
}

/// The adapter every generated API sends through. No default: the app picks
/// its HTTP stack by overriding this provider.
///
/// ```dart
/// ProviderScope(
///   overrides: [
///     flutterRiverpodClientAdapterProvider.overrideWithValue(
///       CratestackHttpAdapter(
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
final flutterRiverpodClientAdapterProvider = Provider<CratestackClientAdapter>((ref) {
  throw UnimplementedError('Override flutterRiverpodClientAdapterProvider before reading the generated CrateStack client.');
});

final flutterRiverpodClientBasePathProvider = Provider<String>((ref) => '/api');

final flutterRiverpodClientClientProvider = Provider<FlutterRiverpodClientCratestackClient>((ref) {
  return FlutterRiverpodClientCratestackClient(
    ref.watch(flutterRiverpodClientAdapterProvider),
    basePath: ref.watch(flutterRiverpodClientBasePathProvider),
  );
});
