// Dio as a `CratestackHttpTransport` (cratestack#1204).
//
// The only file in the generated package that names Dio. Wrap the `Dio` you
// already have (interceptors, base headers, certificate pinning and retry all
// keep working) and hand it to `CratestackHttpAdapter` (REST) or
// `CratestackRpcHttpAdapter` (RPC):
//
//     final adapter = CratestackHttpAdapter(
//       CratestackDioTransport(dio),
//       baseUri: Uri.parse('https://api.example.com'),
//     );
//
// The adapter builds the absolute URI, so `dio.options.baseUrl` is not used.

import 'dart:async';
import 'dart:typed_data';

import 'package:dio/dio.dart';

import 'runtime.dart';

class CratestackDioTransport implements CratestackHttpTransport {
  const CratestackDioTransport(this.dio);

  final Dio dio;

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    final cancelToken = CancelToken();
    var aborted = false;
    final abort = request.abort;
    if (abort != null) {
      unawaited(
        abort.then<void>((_) {
          aborted = true;
          if (!cancelToken.isCancelled) {
            cancelToken.cancel('aborted by the caller');
          }
        }, onError: (Object _) {}),
      );
    }

    final Response<ResponseBody> response;
    try {
      response = await dio.requestUri<ResponseBody>(
        request.uri,
        data: request.body,
        cancelToken: cancelToken,
        options: Options(
          method: request.method,
          headers: request.headers,
          responseType: ResponseType.stream,
          // Every status is an answer, not an error: the adapter maps 4xx/5xx.
          validateStatus: (_) => true,
          extra: request.extra,
        ),
      );
    } on Object catch (error) {
      throw _transportException(error, aborted: aborted);
    }

    return CratestackHttpResponse(
      status: response.statusCode ?? 0,
      headers: <String, String>{
        for (final entry in response.headers.map.entries)
          entry.key: entry.value.join(', '),
      },
      body: _guardBody(response.data!.stream, cancelToken, () => aborted),
    );
  }
}

/// The response body, with its errors mapped to the seam's exceptions and an
/// abort honoured while the body is still streaming. Dio's own cancellation
/// stops a request that is still connecting or waiting for headers, but does
/// not interrupt a body that has started to arrive.
Stream<Uint8List> _guardBody(
  Stream<Uint8List> source,
  CancelToken cancelToken,
  bool Function() aborted,
) {
  late final StreamController<Uint8List> controller;
  StreamSubscription<Uint8List>? subscription;
  controller = StreamController<Uint8List>(
    onListen: () {
      subscription = source.listen(
        controller.add,
        onError: (Object error, StackTrace stackTrace) {
          controller.addError(
            _transportException(error, aborted: aborted()),
            stackTrace,
          );
        },
        onDone: controller.close,
      );
      unawaited(
        cancelToken.whenCancel.then<void>((cancel) {
          if (controller.isClosed) {
            return;
          }
          controller.addError(CratestackTransportAborted(cause: cancel));
          unawaited(subscription?.cancel());
          unawaited(controller.close());
        }),
      );
    },
    onPause: () => subscription?.pause(),
    onResume: () => subscription?.resume(),
    onCancel: () => subscription?.cancel(),
  );
  return controller.stream;
}

/// Maps whatever Dio (or an interceptor) threw to the seam's exceptions.
CratestackTransportException _transportException(
  Object error, {
  required bool aborted,
}) {
  if (error is CratestackTransportException) {
    return error;
  }
  if (aborted) {
    return CratestackTransportAborted(cause: error);
  }
  if (error is! DioException) {
    return CratestackTransportBroken(cause: error);
  }
  // An `if` chain, not an exhaustive `switch`: Dio adds `DioExceptionType`
  // members in minor releases (`transformTimeout` in 5.9), and this package
  // accepts every 5.x from 5.8.0.
  final type = error.type;
  if (type == DioExceptionType.cancel) {
    return CratestackTransportAborted(cause: error);
  }
  // Connect-phase failures: the connect timeout and a rejected certificate.
  // Nothing was written to the server.
  if (type == DioExceptionType.connectionTimeout ||
      type == DioExceptionType.badCertificate) {
    return CratestackTransportUnsent(cause: error);
  }
  if (type == DioExceptionType.connectionError) {
    // Refused, DNS, unreachable: nothing was written. But `dart:io` reports a
    // connection that closed before the response headers as a connection
    // error too, and that happens after the request was written.
    return '${error.error}'.contains('Connection closed before full header')
        ? CratestackTransportBroken(cause: error)
        : CratestackTransportUnsent(cause: error);
  }
  if (type == DioExceptionType.sendTimeout ||
      type == DioExceptionType.receiveTimeout) {
    return CratestackTransportTimedOut(cause: error);
  }
  final inner = error.error;
  return inner is CratestackTransportException
      ? inner
      : CratestackTransportBroken(cause: error);
}
