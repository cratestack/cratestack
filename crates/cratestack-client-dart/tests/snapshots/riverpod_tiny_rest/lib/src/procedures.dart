import 'client.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';
import 'runtime.dart';

part 'procedures.g.dart';

class EchoNameArgs {
  const EchoNameArgs({
required this.name,
  });

  final String name;

  factory EchoNameArgs.fromWire(CratestackValueMap value) {
    return EchoNameArgs(
      name: cratestackRequireWireValue('EchoNameArgs', 'name', value['name']) as String,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'name': name,
    };
  }
}

class ProceduresApi {
  const ProceduresApi(this._client);

  final TinyRestClientCratestackClient _client;

  Future<String> echoName(EchoNameArgs args, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'POST',
      '/\$procs/echoName',
      body: args.toWire(),
      options: options,
    );
    return cratestackRequireWireValue('Procedure', 'echoName', body) as String;
  }

}

final tinyRestClientProceduresApiProvider = Provider<ProceduresApi>((ref) {
  return ref.watch(tinyRestClientClientProvider).procedures;
});

// Issue #302: one `@riverpod` provider per procedure, built by watching
// `tinyRestClientProceduresApiProvider` (declared above) — the
// same existing DI provider `ProceduresApi`'s own methods already go
// through. Query-kind procedures get a plain `Future`-returning function
// (mirrors a model's `get`/`list` providers); mutation-kind procedures
// get a controller class (mirrors a model's create/update/delete
// controllers) — see `model_providers.dart.j2`'s header comment for why
// writes aren't forced into the same shape as reads.
@riverpod
Future<String> echoName(Ref ref, EchoNameArgs args) {
  return ref.watch(tinyRestClientProceduresApiProvider).echoName(args);
}

