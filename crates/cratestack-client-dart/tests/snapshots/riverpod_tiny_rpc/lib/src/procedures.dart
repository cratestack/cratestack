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

  final TinyRpcClientCratestackClient _client;

  Future<String> echoName(EchoNameArgs args, {
    CratestackRpcCallOptions? options,
  }) async {
    final body = await _client.adapter.call(
      'procedure.echoName',
      args.toWire(),
      options: options,
    );
    return cratestackRequireWireValue('Procedure', 'echoName', body) as String;
  }

}

final tinyRpcClientProceduresApiProvider = Provider<ProceduresApi>((ref) {
  return ref.watch(tinyRpcClientClientProvider).procedures;
});
