// Last-resort wall-clock backstop for verify_web_console.dart.
//
// cratestack PR 887's own CI run of the job it hardens hung for the job's
// full 45-minute `timeout-minutes` before the runner force-killed the
// whole process tree, with the tool's own Dart process still alive as an
// orphan. The teardown in chrome_launch.dart (ChromeProcess.shutDown) and
// the explicit exit(code) at the end of main() are meant to make that
// impossible, but "meant to" is exactly the kind of claim a watchdog
// exists to not have to trust: if some future bug reintroduces a leftover
// open handle (a Timer, a StreamSubscription, an HttpClient without
// force-close, ...) that keeps the isolate alive past its own logic
// completing, this Timer fires anyway and force-exits with diagnostics,
// so the failure mode is bounded at `hardTimeoutSeconds`, not the CI job's
// entire budget again.
import 'dart:async';
import 'dart:io';

class HardTimeoutWatchdog {
  HardTimeoutWatchdog(
    Duration timeout, {
    void Function(int code) exitFn = exit,
    void Function(String message) logFn = _defaultLog,
  }) {
    _timer = Timer(timeout, () {
      logFn(
        'FATAL: verify_web_console.dart hit its hard wall-clock timeout '
        '(${timeout.inSeconds}s) without completing on its own. Forcing '
        'exit(2) rather than risk consuming the CI job\'s own timeout '
        '(see cratestack PR 887: a prior version relied on the event loop '
        'draining naturally and hung for 45 minutes when a stray open '
        'handle kept it alive after the marker was already observed).',
      );
      exitFn(2);
    });
  }

  late final Timer _timer;

  /// Must be called once the tool is about to exit on its own — an
  /// uncancelled timer is harmless after `exit()` (the whole isolate is
  /// gone), but cancelling explicitly keeps this class's own contract
  /// simple to reason about and lets it be reused in a test without a
  /// process-level exit.
  void cancel() => _timer.cancel();
}

void _defaultLog(String message) => stderr.writeln(message);
