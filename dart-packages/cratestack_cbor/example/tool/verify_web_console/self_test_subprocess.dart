// Subprocess-level self-tests for `verify_web_console.dart`'s teardown
// (chrome_launch.dart's `ChromeProcess.shutDown`) and hard-timeout
// watchdog (`hard_timeout_watchdog.dart`) — see `self_test.dart` for why
// these exist: cratestack PR 887's own CI run of the job this whole tool
// hardens hung for 45 minutes because the tool's own Dart process never
// exited after printing `PASS:`. These two scenarios run the REAL
// `verify_web_console.dart` entry point as a subprocess (not just its
// internal functions) against a fake DevTools server, because "does the
// whole OS process actually exit" is not observable by calling a Dart
// function in-process — only a subprocess boundary proves it.
// ignore_for_file: avoid_print
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'fake_devtools_server.dart';
import 'port_utils.dart';

/// (a) A fake Chrome that answers DevTools and then keeps running forever
/// (never exits on its own — mirrors a real Chrome subprocess that
/// doesn't die promptly on SIGTERM). Once the marker is observed, the
/// tool process must still exit within 5s with code 0 — proving teardown
/// does not wait for Chrome to exit naturally.
Future<void> scenarioExitsPromptlyAfterMarker() async {
  final port = await pickRelaunchPort(19540);
  final fakeDevtools = await FakeDevToolsServer.start(
    port,
    markerText: 'CRATESTACK_CBOR_EXAMPLE_RESULT: OK deadbeef',
  );
  final fakeChromeScript = await _writeFakeChromeScript();
  Process? tool;
  try {
    tool = await Process.start(
      Platform.resolvedExecutable,
      [
        'run',
        _verifyScriptPath(),
        '--url',
        'http://127.0.0.1:1/dummy',
        '--debug-port',
        '$port'
      ],
      environment: {'CHROME_EXECUTABLE': fakeChromeScript},
    );

    DateTime? passAt;
    final passCompleter = Completer<void>();
    tool.stdout
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen((line) {
      print('[tool stdout] $line');
      if (line.startsWith('PASS:') && !passCompleter.isCompleted) {
        passAt = DateTime.now();
        passCompleter.complete();
      }
    });
    tool.stderr
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen((line) => print('[tool stderr] $line'));

    await passCompleter.future.timeout(const Duration(seconds: 30));
    final exitCode = await tool.exitCode.timeout(const Duration(seconds: 5));
    final elapsed = DateTime.now().difference(passAt!);
    if (exitCode != 0) {
      throw StateError('expected exit code 0 after PASS, got $exitCode');
    }
    print(
      '[self-test 3/4] PASS: tool exited $elapsed after PASS with code '
      '$exitCode despite the fake Chrome process staying alive',
    );
  } finally {
    tool?.kill(ProcessSignal.sigkill);
    await fakeDevtools.close();
  }
}

/// (b) A fake DevTools server that accepts the connection but never sends
/// the marker. With a small `--hard-timeout-seconds`, the watchdog must
/// fire and the tool must exit with code 2 — proving the watchdog is a
/// real backstop, not dead code.
Future<void> scenarioWatchdogFiresOnNoMarker() async {
  final port = await pickRelaunchPort(19541);
  final fakeDevtools = await FakeDevToolsServer.start(port); // no markerText
  final fakeChromeScript = await _writeFakeChromeScript();
  Process? tool;
  try {
    tool = await Process.start(
      Platform.resolvedExecutable,
      [
        'run',
        _verifyScriptPath(),
        '--url', 'http://127.0.0.1:1/dummy',
        '--debug-port', '$port',
        '--timeout-seconds', '30', // marker wait — must NOT be what fires
        '--hard-timeout-seconds', '2', // watchdog — must be what fires
      ],
      environment: {'CHROME_EXECUTABLE': fakeChromeScript},
    );
    final stderrLines = <String>[];
    tool.stderr
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen((line) {
      print('[tool stderr] $line');
      stderrLines.add(line);
    });
    tool.stdout.transform(utf8.decoder).listen((_) {});

    final started = DateTime.now();
    final exitCode = await tool.exitCode.timeout(const Duration(seconds: 15));
    final elapsed = DateTime.now().difference(started);
    if (exitCode != 2) {
      throw StateError('expected watchdog exit code 2, got $exitCode');
    }
    if (!stderrLines.any((l) => l.contains('hard wall-clock timeout'))) {
      throw StateError(
        'expected watchdog diagnostic on stderr, got: $stderrLines',
      );
    }
    print(
      '[self-test 4/4] PASS: watchdog fired after $elapsed, tool exited '
      'with code 2 and a diagnostic',
    );
  } finally {
    tool?.kill(ProcessSignal.sigkill);
    await fakeDevtools.close();
  }
}

String _verifyScriptPath() {
  final selfTestDir = File.fromUri(Platform.script).parent;
  return File('${selfTestDir.parent.path}/verify_web_console.dart').path;
}

Future<String> _writeFakeChromeScript() async {
  final dir = await Directory.systemTemp.createTemp('cbor_verify_selftest_');
  final script = File('${dir.path}/fake_chrome.sh');
  // Ignores every argument verify_web_console.dart's launcher passes
  // (--headless=new etc.) and just stays alive — this is the "Chrome
  // subprocess that doesn't die promptly" this scenario needs.
  await script.writeAsString('#!/bin/sh\nexec sleep 300\n');
  await Process.run('chmod', ['+x', script.path]);
  return script.path;
}
