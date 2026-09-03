// Manual proof for `devtools_ready.dart`'s two root-cause fixes, run with:
//
//   dart run tool/verify_web_console/self_test.dart
//
// Deliberately NOT wired into `flutter test` / `just cbor-example-verify`:
// scenario 2 below needs ~20 real seconds to demonstrate that the old 15s
// deadline would have failed, and paying that on every CI run (this
// package's `flutter test` step already runs on every `cbor-example-
// verify` invocation) is not worth it for a fix that is otherwise fully
// exercised by the fast scenario 1 and by the real headless-Chrome run
// `cbor-example-verify` already does. Run this manually after touching
// `devtools_ready.dart`.
// ignore_for_file: avoid_print
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'chrome_stderr_capture.dart';
import 'devtools_ready.dart';
import 'port_utils.dart';

Future<void> main() async {
  await _scenarioExitingChromeSurfacesStderrAndExitCode();
  await _scenarioSlowServerBeatsOldDeadlineButNotNewOne();
  print('ALL SELF-TESTS PASSED');
}

/// Root cause (b)+(c): a fake "chrome" that exits immediately with stderr
/// output must fail fast (not wait out the deadline) with an error that
/// contains what it printed and its exit code.
Future<void> _scenarioExitingChromeSurfacesStderrAndExitCode() async {
  final port = await pickRelaunchPort(19333);
  final process = await Process.start('/bin/sh', [
    '-c',
    'echo boom >&2; exit 3',
  ]);
  final capture = ChromeStderrCapture();
  process.stderr.transform(utf8.decoder).listen(capture.add);

  final started = DateTime.now();
  Object? caught;
  try {
    await waitForDevtoolsReady(
      process: process,
      port: port,
      stderrCapture: capture,
      timeout: const Duration(seconds: 10),
      pollInterval: const Duration(milliseconds: 100),
    );
  } catch (e) {
    caught = e;
  }
  final elapsed = DateTime.now().difference(started);

  if (caught == null) {
    throw StateError('expected waitForDevtoolsReady to fail — it did not');
  }
  final message = caught.toString();
  if (!message.contains('boom')) {
    throw StateError('error message missing Chrome stderr ("boom"): $message');
  }
  if (!message.contains('exit code 3')) {
    throw StateError('error message missing exit code: $message');
  }
  if (elapsed >= const Duration(seconds: 5)) {
    throw StateError(
      'expected the exited-process check to fail fast, took $elapsed',
    );
  }
  print('[self-test 1/2] PASS ($elapsed): $message');
}

/// Root cause (a): a server that only becomes ready after 20s must fail
/// under the OLD 15s deadline and SUCCEED under the new 60s default —
/// proving the raised deadline is what fixes the flake, not some other
/// side effect.
Future<void> _scenarioSlowServerBeatsOldDeadlineButNotNewOne() async {
  final port = await pickRelaunchPort(19334);
  // A long-lived, never-exiting fake "chrome" so this scenario is purely
  // about the polling deadline, not the exited-process path scenario 1
  // already covers.
  final process = await Process.start('/bin/sh', ['-c', 'sleep 40']);
  final capture = ChromeStderrCapture();
  process.stderr.transform(utf8.decoder).listen(capture.add);

  HttpServer? server;
  unawaited(
    Future<void>.delayed(const Duration(seconds: 20), () async {
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, port);
      server!.listen((request) {
        request.response
          ..statusCode = 200
          ..write('{}');
        unawaited(request.response.close());
      });
    }),
  );

  try {
    Object? oldDeadlineFailure;
    try {
      await waitForDevtoolsReady(
        process: process,
        port: port,
        stderrCapture: capture,
        timeout: const Duration(seconds: 15), // the OLD hardcoded value
        pollInterval: const Duration(milliseconds: 200),
      );
    } catch (e) {
      oldDeadlineFailure = e;
    }
    if (oldDeadlineFailure == null) {
      throw StateError(
        'expected the old 15s deadline to fail against a 20s-late server',
      );
    }
    print('[self-test 2/2] old 15s deadline fails as expected: '
        '$oldDeadlineFailure');

    // New default: same server (now up, or up shortly), generous deadline.
    await waitForDevtoolsReady(
      process: process,
      port: port,
      stderrCapture: capture,
      timeout: const Duration(seconds: 60),
      pollInterval: const Duration(milliseconds: 250),
    );
    print('[self-test 2/2] PASS: new 60s default succeeds against the '
        'same 20s-late server');
  } finally {
    process.kill();
    await server?.close(force: true);
  }
}
