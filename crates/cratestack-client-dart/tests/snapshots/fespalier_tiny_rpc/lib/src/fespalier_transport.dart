// fespalier's `HttpTransport` as a `CratestackHttpTransport` (cratestack#1204).
//
// The only file in the generated package that names `fespalier_transport`
// besides the provider file. Hand the adapter your transport chain once and
// the generated client sends through it:
//
//     final adapter = CratestackRpcHttpAdapter(
//       CratestackFespalierTransport(
//         SessionTransport(WriteGuardTransport(HttpClientTransport(client)), credentials),
//       ),
//       baseUri: Uri.parse('https://api.example.com'),
//     );
//
// The Riverpod providers do this for you: override
// `tinyRpcClientTransportProvider` with the chain and
// `tinyRpcClientBaseUriProvider` with the server's origin.

import 'dart:async';
import 'dart:typed_data';

import 'package:fespalier_transport/fespalier_transport.dart';

import 'runtime.dart';

class CratestackFespalierTransport implements CratestackHttpTransport {
  const CratestackFespalierTransport(this.transport);

  final HttpTransport transport;

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    final TransportResponse response;
    try {
      response = await transport.send(
        TransportRequest(
          method: request.method,
          uri: request.uri,
          headers: request.headers,
          // The exact bytes the adapter encoded: a signer layered above sees
          // these and nothing else.
          body: request.body,
          // Either aborts: the call option's `abort` and the abort trigger of
          // the zone the call runs in, merged. The zone's is how
          // `ref.cancellable` in `fespalier_cratestack` aborts a page's loads
          // without extra wiring, even for a call that carries its own abort.
          abortTrigger: TransportZone.merge(
            request.abort,
            TransportZone.current(),
          ),
          attributes: request.extra,
        ),
      );
    } on TransportFailure catch (failure) {
      throw _exceptionFor(failure);
    } on TransportStatusError catch (error) {
      // A decorator that wraps error answers in exceptions (as some HTTP
      // clients do for a 4xx or a 5xx) still carries an answer: hand it to the
      // adapter, which maps it to `CratestackRestException` /
      // `CratestackRpcException`.
      return CratestackHttpResponse.buffered(
        status: error.statusCode,
        headers: error.headers,
        body: error.body,
      );
    }
    return CratestackHttpResponse(
      status: response.statusCode,
      headers: response.headers,
      body: response.body.transform(
        StreamTransformer<List<int>, Uint8List>.fromHandlers(
          handleData: (chunk, sink) => sink.add(
            chunk is Uint8List ? chunk : Uint8List.fromList(chunk),
          ),
          handleError: (error, stackTrace, sink) => sink.addError(
            error is TransportFailure ? _exceptionFor(error) : error,
            stackTrace,
          ),
        ),
      ),
    );
  }
}

/// Maps a `TransportFailure` to the seam's exceptions. [TransportPortal] and
/// [TransportUntrusted] have no kind of their own in the seam, so they become
/// [CratestackTransportBroken] (a portal's answer may have reached the server,
/// and a refused certificate is not "offline"); the `cause` of every mapped
/// exception is the original failure, so a reader that must tell them apart
/// checks `error.cause is TransportPortal` / `TransportUntrusted`.
CratestackTransportException _exceptionFor(TransportFailure failure) {
  return switch (failure) {
    TransportAborted() => CratestackTransportAborted(cause: failure),
    TransportUnsent() => CratestackTransportUnsent(cause: failure),
    TransportTimedOut() => CratestackTransportTimedOut(cause: failure),
    TransportBroken() ||
    TransportPortal() ||
    TransportUntrusted() => CratestackTransportBroken(cause: failure),
  };
}
