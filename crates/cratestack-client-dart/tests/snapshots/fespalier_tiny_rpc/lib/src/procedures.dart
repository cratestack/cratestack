import 'client.dart';
import 'package:cratestack_annotations/cratestack_annotations.dart';
import 'package:dart_mappable/dart_mappable.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';
import 'runtime.dart';

part 'procedures.g.dart';
// issue #325: gated (unlike `part_file_name` above) — see
// `rest_procedures.dart.j2`'s identical guard for why.
part 'procedures.mapper.dart';
// issue #668 phase 2: same gate, same reason — see
// `rest_procedures.dart.j2`'s identical guard for why.
part 'procedures.builder.dart';

// issue #325: `@MappableClass()` (expanded by `dart_mappable_builder`
// alongside `riverpod_generator` in the same `build_runner` pass) gives
// this class real `operator ==`/`hashCode`/`copyWith` — every generated
// data class needs this under the `riverpod` preset because any of them
// can end up as a `@riverpod` family provider's argument type (see
// `EstimateFocusMinutesArgs` in `procedures.dart`), and riverpod's family
// cache dedupes provider instances by argument *value* equality, not
// identity. `generateMethods: GenerateMethods.equals | GenerateMethods.copy`
// deliberately does NOT ask for `encode`/`decode`/`stringify` — this
// generator already hand-rolls `fromWire`/`toWire` below (different
// method names, so there's no collision either way), and duplicating a
// second, unused `toMap`/`fromMap`/`toJson`/`fromJson` surface per class
// would be pure noise. Relation fields (e.g. `Task.board` -> `Board?`)
// get deep equality "for free": the referenced type is itself a
// `@MappableClass()`-annotated generated class in this same preset, so
// the field-by-field comparison this class's own `==` performs recurses
// into the related type's generated `==` rather than falling back to
// object identity; list-valued relations get the same element-wise
// (not `List.==`/identity) comparison automatically from
// `dart_mappable`'s own list handling.
@MappableClass(generateMethods: GenerateMethods.equals | GenerateMethods.copy)
@CratestackBuilder()
class EchoNameArgs with EchoNameArgsMappable {
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

// Issue #302: one `@riverpod` provider per procedure, built by watching
// `tinyRpcClientProceduresApiProvider` (declared above) — the
// same existing DI provider `ProceduresApi`'s own methods already go
// through. Query-kind procedures get a plain `Future`-returning function
// (mirrors a model's `get`/`list` providers); mutation-kind procedures
// get a controller class (mirrors a model's create/update/delete
// controllers) — see `model_providers.dart.j2`'s header comment for why
// writes aren't forced into the same shape as reads.
@riverpod
Future<String> echoName(Ref ref, EchoNameArgs args) {
  return ref.watch(tinyRpcClientProceduresApiProvider).echoName(args);
}

