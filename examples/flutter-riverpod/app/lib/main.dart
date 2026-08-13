import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod_client/flutter_riverpod_client.dart';

import 'src/runtime.dart';
import 'src/screens/boards_screen.dart';

void main() {
  runApp(
    ProviderScope(
      overrides: [
        // The one override every consumer of this generated package
        // must supply — see `client/README.md`'s "Adapter Setup" /
        // "Riverpod Setup" sections. Every `@riverpod` provider in
        // `client/` (`board`, `boardList`, `BoardCreateController`, ...)
        // is built by watching `flutterRiverpodClientBoardApiProvider`,
        // which itself watches this one — overriding it alone is
        // enough to point the *entire* generated surface at a real
        // server, which is exactly what this override demonstrates.
        flutterRiverpodClientAdapterProvider.overrideWithValue(
          CratestackDioAdapter(dio: buildAppDio()),
        ),
      ],
      child: const RiverpodExampleApp(),
    ),
  );
}

class RiverpodExampleApp extends StatelessWidget {
  const RiverpodExampleApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'CrateStack Riverpod Example',
      theme: ThemeData(colorSchemeSeed: Colors.indigo, useMaterial3: true),
      home: const BoardsScreen(),
    );
  }
}
