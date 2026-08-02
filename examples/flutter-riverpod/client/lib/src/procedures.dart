import 'client.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';
import 'runtime.dart';

part 'procedures.g.dart';

class EstimateFocusMinutesArgs {
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

class FocusEstimateArgs {
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

class FocusEstimateResult {
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

