// Loopback port helpers for `verify_web_console.dart`'s Chrome-relaunch
// path (see `devtools_ready.dart`). Split out because the relaunch
// contract ("same port after confirming it's free, or a new port passed
// through") is a self-contained, independently testable piece of
// bookkeeping.
import 'dart:io';

/// True if [port] can be bound on loopback right now, i.e. nothing (not
/// even a lingering TIME_WAIT socket from a just-killed Chrome) is holding
/// it.
Future<bool> isPortFree(int port) async {
  try {
    final socket = await ServerSocket.bind(InternetAddress.loopbackIPv4, port);
    await socket.close();
    return true;
  } on SocketException {
    return false;
  }
}

/// Picks the port a relaunch attempt should use: [preferredPort] itself if
/// it frees up within [confirmTimeout] (the common case — the just-killed
/// Chrome's listener closes quickly), otherwise the next port up. Falling
/// back instead of blocking indefinitely means a relaunch attempt is never
/// stuck behind a port some unrelated process is holding.
Future<int> pickRelaunchPort(
  int preferredPort, {
  Duration confirmTimeout = const Duration(seconds: 3),
  Duration pollInterval = const Duration(milliseconds: 100),
}) async {
  final deadline = DateTime.now().add(confirmTimeout);
  while (DateTime.now().isBefore(deadline)) {
    if (await isPortFree(preferredPort)) {
      return preferredPort;
    }
    await Future<void>.delayed(pollInterval);
  }
  return preferredPort + 1;
}
