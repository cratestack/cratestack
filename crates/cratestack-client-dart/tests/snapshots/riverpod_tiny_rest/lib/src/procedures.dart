import 'client.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'runtime.dart';

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
