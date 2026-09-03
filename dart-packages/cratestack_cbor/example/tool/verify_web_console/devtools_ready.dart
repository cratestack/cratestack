// DevTools-readiness waiting and one-retry relaunch for
// `verify_web_console.dart`.
//
// Fixes three root causes behind a CI flake that failed
// `flutter (cratestack_cbor example — linux + web, real builds)` three
// times in one day (runs 33694021063, 33771671818, 33772875028) with every
// real build step green, always ~15s after Chrome launched, always
// clearing on a plain rerun:
//
//   (a) the old 15s deadline was too tight on a loaded CI runner —
//       [waitForDevtoolsReady] now takes a caller-supplied timeout
//       (`verify_web_console.dart`'s `--devtools-ready-timeout-seconds`,
//       default 60) and polls every 250ms instead of 200ms/15s.
//   (b) Chrome's stderr used to be discarded — [ChromeStderrCapture] (see
//       `chrome_stderr_capture.dart`) is now threaded through every
//       failure path, so the thrown [StateError] always says what Chrome
//       said.
//   (c) nothing checked whether Chrome had already exited — this now
//       races the poll loop against `process.exitCode` so a dead Chrome
//       fails immediately with its real exit code instead of waiting out
//       the full deadline first.
//
// [ensureChromeReady] adds one automatic relaunch: a first readiness
// failure is logged loudly and retried once with a fresh Chrome process
// (same port if it frees up, otherwise the next port — see
// `port_utils.dart`); a second failure is fatal with full diagnostics from
// both attempts.
//
// Every Chrome teardown here goes through `ChromeProcess.shutDown`
// (chrome_launch.dart), never a bare `process.kill()` — that fix's own
// first landing hung PR 887's own CI run for the job's full 45-minute
// timeout, because `waitForDevtoolsReady` reading `process.exitCode` (for
// root cause (c) above) opens a native exit-watch handle that keeps the
// Dart isolate alive until the process is truly reaped, and a bare
// `kill()` (SIGTERM) doesn't guarantee that. `HttpClient.close(force:
// true)` here is the same defense for the readiness poller's own HTTP
// connections.
import 'dart:async';
import 'dart:io';

import 'chrome_launch.dart';
import 'chrome_stderr_capture.dart';
import 'port_utils.dart';

/// Polls `http://127.0.0.1:$port/json/version` until it answers 200, the
/// [timeout] elapses, or [process] exits first (checked immediately, not
/// only once the deadline is reached — root cause (c) above). Throws a
/// [StateError] whose message always includes [stderrCapture]'s tail and,
/// if the process exited, its exit code — root cause (b).
Future<void> waitForDevtoolsReady({
  required Process process,
  required int port,
  required ChromeStderrCapture stderrCapture,
  Duration timeout = const Duration(seconds: 60),
  Duration pollInterval = const Duration(milliseconds: 250),
}) async {
  final client = HttpClient();
  int? exitCode;
  // A Future may be awaited/`.then`-ed any number of times, so this does
  // not consume anything later callers need.
  unawaited(process.exitCode.then((code) => exitCode = code));

  void checkNotExited() {
    if (exitCode != null) {
      throw StateError(
        'Chrome process exited before DevTools Protocol became ready on '
        'port $port (exit code $exitCode). Chrome stderr:\n'
        '${stderrCapture.tail}',
      );
    }
  }

  try {
    final deadline = DateTime.now().add(timeout);
    while (DateTime.now().isBefore(deadline)) {
      checkNotExited();
      try {
        final request = await client
            .getUrl(Uri.parse('http://127.0.0.1:$port/json/version'))
            .timeout(pollInterval);
        final response = await request.close().timeout(pollInterval);
        if (response.statusCode == 200) {
          await response.drain<void>();
          return;
        }
      } catch (_) {
        // Not up yet (connection refused, timed out, non-200) — retry.
      }
      await Future.any<void>([
        Future<void>.delayed(pollInterval),
        process.exitCode.then((_) {}),
      ]);
    }
    checkNotExited();
    throw StateError(
      'Chrome DevTools Protocol never became ready on port $port within '
      '${timeout.inSeconds}s. Chrome stderr:\n${stderrCapture.tail}',
    );
  } finally {
    // `force: true`: don't wait for any keep-alive connection this client
    // opened to close on its own — see this file's module doc.
    client.close(force: true);
  }
}

/// Launches Chrome on [initialPort] and waits for DevTools readiness,
/// retrying once (fresh process, loudly logged) on failure. Rethrows the
/// second attempt's [StateError] — carrying that attempt's own
/// diagnostics — if it also fails. Every failure path tears its Chrome
/// process down via [ChromeProcess.shutDown] before returning or retrying.
Future<ChromeProcess> ensureChromeReady({
  required int initialPort,
  String? chromeBinary,
  Duration timeout = const Duration(seconds: 60),
  Duration pollInterval = const Duration(milliseconds: 250),
  void Function(String) log = print,
}) async {
  var port = initialPort;
  var chrome = await launchChrome(port: port, chromeBinary: chromeBinary);
  try {
    await waitForDevtoolsReady(
      process: chrome.process,
      port: port,
      stderrCapture: chrome.stderrCapture,
      timeout: timeout,
      pollInterval: pollInterval,
    );
    return chrome;
  } catch (firstFailure) {
    log('=== Chrome DevTools readiness failed once: $firstFailure ===');
    log('=== relaunching Chrome once and retrying readiness ===');
    await chrome.shutDown();
    port = await pickRelaunchPort(port);
    chrome = await launchChrome(port: port, chromeBinary: chromeBinary);
    try {
      await waitForDevtoolsReady(
        process: chrome.process,
        port: port,
        stderrCapture: chrome.stderrCapture,
        timeout: timeout,
        pollInterval: pollInterval,
      );
      return chrome;
    } catch (secondFailure) {
      await chrome.shutDown();
      throw StateError(
        'Chrome DevTools readiness failed twice in a row — giving up.\n'
        'First attempt (port $initialPort): $firstFailure\n'
        'Second attempt (port $port): $secondFailure',
      );
    }
  }
}
