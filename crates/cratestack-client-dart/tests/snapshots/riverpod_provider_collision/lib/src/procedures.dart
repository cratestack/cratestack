import 'client.dart';
import 'models/widget.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';
import 'runtime.dart';

part 'procedures.g.dart';

class WidgetCreateArgs {
  const WidgetCreateArgs({
required this.name,
  });

  final String name;

  factory WidgetCreateArgs.fromWire(CratestackValueMap value) {
    return WidgetCreateArgs(
      name: cratestackRequireWireValue('WidgetCreateArgs', 'name', value['name']) as String,
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

  final DartVerifyRiverpodCollisionCratestackClient _client;

  Future<Widget> widgetCreate(WidgetCreateArgs args, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'POST',
      '/\$procs/widgetCreate',
      body: args.toWire(),
      options: options,
    );
    return Widget.fromWire(cratestackAsValueMap(cratestackRequireWireValue('Procedure', 'widgetCreate', body)));
  }

}

final dartVerifyRiverpodCollisionProceduresApiProvider = Provider<ProceduresApi>((ref) {
  return ref.watch(dartVerifyRiverpodCollisionClientProvider).procedures;
});

// Issue #302: one `@riverpod` provider per procedure, built by watching
// `dartVerifyRiverpodCollisionProceduresApiProvider` (declared above) — the
// same existing DI provider `ProceduresApi`'s own methods already go
// through. Query-kind procedures get a plain `Future`-returning function
// (mirrors a model's `get`/`list` providers); mutation-kind procedures
// get a controller class (mirrors a model's create/update/delete
// controllers) — see `model_providers.dart.j2`'s header comment for why
// writes aren't forced into the same shape as reads.
@riverpod
class DartVerifyRiverpodCollisionWidgetCreateController extends _$DartVerifyRiverpodCollisionWidgetCreateController {
  @override
  FutureOr<Widget?> build() => null;

  Future<Widget> widgetCreate(WidgetCreateArgs args) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(dartVerifyRiverpodCollisionProceduresApiProvider).widgetCreate(args);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}

