// Starts a headless Chrome process for verify_web_console.dart, wired to
// a ChromeStderrCapture instead of the old discard-everything listener
// (process.stderr.transform(utf8.decoder).listen((_) {})) -- that silent
// drop is exactly why the CI flake this file's introduction fixes shipped
// with zero diagnostics three times in a row.
//
// ChromeProcess.shutDown exists because of a SECOND, worse failure this
// file's own fix introduced and had to close (cratestack PR 887's own CI
// run): a live Process.stdout/stderr StreamSubscription, or a
// process.exitCode watcher, is an "active handle" that keeps the Dart
// isolate from exiting naturally -- the old script never touched
// process.exitCode at all, so it always drained cleanly, but this
// module's readiness-vs-exit race (devtools_ready.dart) does, and a
// Chrome subprocess that doesn't fully die on SIGTERM (a lingering
// renderer/zygote holding the stdout/stderr pipe's write end open) turned
// a 15s flake into a 45-minute hang until the CI job's own timeout killed
// the whole process tree. shutDown cancels both subscriptions and
// escalates to SIGKILL if SIGTERM doesn't reap the process in time -- see
// verify_web_console.dart's explicit exit(code) and hard-timeout watchdog
// for why this is defense in depth, not the only fix.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'chrome_stderr_capture.dart';

/// A running Chrome process plus the bounded stderr capture wired to it.
/// Bundled together so a caller cannot forget to attach the listener
/// before the process starts producing output, and so teardown
/// (shutDown) always has the subscriptions it needs to cancel.
class ChromeProcess {
  ChromeProcess(
    this.process,
    this.stderrCapture,
    this.port,
    this._stdoutSub,
    this._stderrSub,
  );

  final Process process;
  final ChromeStderrCapture stderrCapture;

  /// The DevTools Protocol port this process was launched with. Callers
  /// must read this rather than assuming the port they originally asked
  /// for: ensureChromeReady's relaunch attempt may have picked a
  /// different one if the original port hadn't freed up yet.
  final int port;

  final StreamSubscription<String> _stdoutSub;
  final StreamSubscription<String> _stderrSub;

  /// Tears this process down deterministically: cancels the stdout/stderr
  /// subscriptions, sends SIGTERM, and escalates to SIGKILL if the process
  /// hasn't exited within killGrace. Must be called on every exit path
  /// (success, failure, or relaunch) -- see this file's module doc.
  Future<void> shutDown({
    Duration killGrace = const Duration(seconds: 5),
  }) async {
    await _stdoutSub.cancel();
    await _stderrSub.cancel();
    process.kill();
    if (await _exitedWithin(killGrace)) {
      return;
    }
    process.kill(ProcessSignal.sigkill);
    // Best-effort: give SIGKILL a moment to be reaped too, but never block
    // indefinitely on it -- verify_web_console.dart's hard-timeout
    // watchdog is the guaranteed final backstop if even this fails.
    await _exitedWithin(killGrace);
  }

  Future<bool> _exitedWithin(Duration timeout) async {
    try {
      await process.exitCode.timeout(timeout);
      return true;
    } on TimeoutException {
      return false;
    }
  }
}

/// Launches headless Chrome with its DevTools Protocol port bound to
/// port. chromeBinary defaults to the CHROME_EXECUTABLE environment
/// variable (unchanged from before this split), falling back to
/// google-chrome.
Future<ChromeProcess> launchChrome({
  required int port,
  String? chromeBinary,
}) async {
  final binary = chromeBinary ??
      Platform.environment['CHROME_EXECUTABLE'] ??
      'google-chrome';
  final process = await Process.start(binary, [
    '--headless=new',
    '--disable-gpu',
    '--no-sandbox',
    '--remote-debugging-port=$port',
    '--remote-debugging-address=127.0.0.1',
    'about:blank',
  ]);

  final stderrCapture = ChromeStderrCapture();
  final stdoutSub = process.stdout.transform(utf8.decoder).listen((_) {});
  final stderrSub =
      process.stderr.transform(utf8.decoder).listen(stderrCapture.add);
  return ChromeProcess(process, stderrCapture, port, stdoutSub, stderrSub);
}
