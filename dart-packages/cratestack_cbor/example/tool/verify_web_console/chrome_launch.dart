// Starts a headless Chrome process for `verify_web_console.dart`, wired to
// a [ChromeStderrCapture] instead of the old discard-everything listener
// (`process.stderr.transform(utf8.decoder).listen((_) {})`) — that silent
// drop is exactly why the CI flake this file's introduction fixes shipped
// with zero diagnostics three times in a row.
import 'dart:convert';
import 'dart:io';

import 'chrome_stderr_capture.dart';

/// A running Chrome process plus the bounded stderr capture wired to it.
/// Bundled together so a caller cannot forget to attach the listener
/// before the process starts producing output.
class ChromeProcess {
  ChromeProcess(this.process, this.stderrCapture, this.port);

  final Process process;
  final ChromeStderrCapture stderrCapture;

  /// The DevTools Protocol port this process was launched with. Callers
  /// must read this rather than assuming the port they originally asked
  /// for: `ensureChromeReady`'s relaunch attempt may have picked a
  /// different one if the original port hadn't freed up yet.
  final int port;
}

/// Launches headless Chrome with its DevTools Protocol port bound to
/// [port]. [chromeBinary] defaults to the `CHROME_EXECUTABLE` environment
/// variable (unchanged from before this split), falling back to
/// `google-chrome`.
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
  process.stdout.transform(utf8.decoder).listen((_) {});
  process.stderr.transform(utf8.decoder).listen(stderrCapture.add);
  return ChromeProcess(process, stderrCapture, port);
}
