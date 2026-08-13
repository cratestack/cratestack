import 'client.dart';
import 'package:dart_mappable/dart_mappable.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';
import 'runtime.dart';

part 'procedures.g.dart';
// issue #325: gated (unlike `part_file_name` above) because
// `dart_mappable_builder`, unlike `riverpod_generator`, skips writing a
// part file entirely rather than emitting an empty no-op one when its
// target file has zero `@MappableClass()` classes in it — a schema with
// zero procedures and no procedure-owned nested `type`s would otherwise
// hit a real `uri_does_not_exist` `flutter analyze` error (confirmed
// empirically — see `shared_types.dart.j2`'s identical guard).
part 'procedures.mapper.dart';

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
class EstimateFocusMinutesArgs with EstimateFocusMinutesArgsMappable {
  const EstimateFocusMinutesArgs({
required this.args,
  });

  final FocusEstimateArgs args;

  factory EstimateFocusMinutesArgs.fromWire(CratestackValueMap value) {
    return EstimateFocusMinutesArgs(
      args: FocusEstimateArgs.fromWire(cratestackAsValueMap(cratestackRequireWireValue('EstimateFocusMinutesArgs', 'args', value['args']))),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'args': args.toWire(),
    };
  }
}

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
class FocusEstimateArgs with FocusEstimateArgsMappable {
  const FocusEstimateArgs({
required this.taskCount,
required this.minutesPerTask,
  });

  final int taskCount;
  final int minutesPerTask;

  factory FocusEstimateArgs.fromWire(CratestackValueMap value) {
    return FocusEstimateArgs(
      taskCount: (cratestackRequireWireValue('FocusEstimateArgs', 'taskCount', value['taskCount']) as num).toInt(),
      minutesPerTask: (cratestackRequireWireValue('FocusEstimateArgs', 'minutesPerTask', value['minutesPerTask']) as num).toInt(),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'taskCount': taskCount,
      'minutesPerTask': minutesPerTask,
    };
  }
}

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
class FocusEstimateResult with FocusEstimateResultMappable {
  const FocusEstimateResult({
required this.totalMinutes,
  });

  final int totalMinutes;

  factory FocusEstimateResult.fromWire(CratestackValueMap value) {
    return FocusEstimateResult(
      totalMinutes: (cratestackRequireWireValue('FocusEstimateResult', 'totalMinutes', value['totalMinutes']) as num).toInt(),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'totalMinutes': totalMinutes,
    };
  }
}

class ProceduresApi {
  const ProceduresApi(this._client);

  final FlutterRiverpodClientCratestackClient _client;

  Future<FocusEstimateResult> estimateFocusMinutes(EstimateFocusMinutesArgs args, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'POST',
      '/\$procs/estimateFocusMinutes',
      body: args.toWire(),
      options: options,
    );
    return FocusEstimateResult.fromWire(cratestackAsValueMap(cratestackRequireWireValue('Procedure', 'estimateFocusMinutes', body)));
  }

}

final flutterRiverpodClientProceduresApiProvider = Provider<ProceduresApi>((ref) {
  return ref.watch(flutterRiverpodClientClientProvider).procedures;
});

// Issue #302: one `@riverpod` provider per procedure, built by watching
// `flutterRiverpodClientProceduresApiProvider` (declared above) — the
// same existing DI provider `ProceduresApi`'s own methods already go
// through. Query-kind procedures get a plain `Future`-returning function
// (mirrors a model's `get`/`list` providers); mutation-kind procedures
// get a controller class (mirrors a model's create/update/delete
// controllers) — see `model_providers.dart.j2`'s header comment for why
// writes aren't forced into the same shape as reads.
@riverpod
Future<FocusEstimateResult> estimateFocusMinutes(Ref ref, EstimateFocusMinutesArgs args) {
  return ref.watch(flutterRiverpodClientProceduresApiProvider).estimateFocusMinutes(args);
}

