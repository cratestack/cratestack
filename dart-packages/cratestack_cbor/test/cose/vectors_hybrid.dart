// Runs on the VM next to a browser test and hands it the shared vector
// files, which a browser cannot read: no vector file is copied into the
// package.
import 'dart:io';

import 'package:stream_channel/stream_channel.dart';

void hybridMain(StreamChannel<Object?> channel) {
  const dir = '../../crates/cratestack-cose/tests/vectors';
  channel.sink
    ..add(File('$dir/keys.json').readAsStringSync())
    ..add(File('$dir/unary.json').readAsStringSync())
    ..close();
}
