/// `build_runner` entry point — referenced from `build.yaml`'s
/// `builders.cratestack_builder.import` key, never imported by consumers
/// directly.
library;

import 'package:build/build.dart';
import 'package:source_gen/source_gen.dart';

import 'src/builder_generator.dart';

/// Wraps [CratestackBuilderGenerator] in a [PartBuilder] so consumers write
/// `part '<name>.builder.dart';` in the annotated file and `source_gen`
/// takes care of emitting (and validating) the matching `part of` header —
/// no hand-rolled string-prepending needed, unlike the lean_builder spike.
Builder cratestackBuilder(BuilderOptions options) =>
    PartBuilder([const CratestackBuilderGenerator()], '.builder.dart');
