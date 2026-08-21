import 'package:analyzer/dart/element/element.dart';
import 'package:analyzer/dart/element/nullability_suffix.dart';
import 'package:analyzer/dart/element/type.dart';
import 'package:build/build.dart';
import 'package:cratestack_annotations/cratestack_annotations.dart';
import 'package:source_gen/source_gen.dart';

/// Generates a fluent builder class for every `@CratestackBuilder()`
/// annotated class in a library, reproducing (byte-for-byte, modulo
/// whitespace) what `crates/cratestack-client-dart`'s
/// `model_builder_class.dart.j2` template emits inline today.
///
/// SPIKE FINDING (ported from the lean_builder prototype, unchanged): every
/// decision this generator makes — which fields are "builder required",
/// which are lists, what the list element type is, whether a `build`
/// collision shim is needed — is derived purely from the Dart
/// `ClassElement`/`ConstructorElement`/`DartType` already present on the
/// annotated class — with exactly ONE exception, `listDefaults`.
///
/// That exception is load-bearing and was found the hard way: a projection
/// model's list field and a patch (`Update{Model}Input`) list field emit
/// BYTE-IDENTICAL Dart — `this.tags` plus `final List<String>? tags;` in
/// both cases. Patch-ness is therefore not recoverable from the generated
/// source at all. An earlier revision inferred it from nullability, which
/// made model builders emit `null` where Rust emits `[]` and regressed
/// cratestack#661's own committed regression test. The Rust generator knows
/// `is_patch` and is what emits this annotation, so it passes the answer in.
///
/// Required-ness, by contrast, IS recoverable (`param.isRequiredNamed`) and
/// is never threaded through.
///
/// Unlike the lean_builder version, this generator does NOT hand-prepend a
/// `part of` header: it is wrapped in [PartBuilder] (see `../builder.dart`),
/// which emits the `part of '<file>.dart';` directive itself and validates
/// that the input file actually declares the matching `part` directive.
class CratestackBuilderGenerator extends GeneratorForAnnotation<CratestackBuilder> {
  const CratestackBuilderGenerator();

  @override
  String generateForAnnotatedElement(
    Element element,
    ConstantReader annotation,
    BuildStep buildStep,
  ) {
    // The one piece of schema knowledge the emitted Dart cannot carry.
    final listDefaults = annotation.read('listDefaults').boolValue;

    if (element is! ClassElement) {
      throw InvalidGenerationSourceError(
        '@CratestackBuilder() can only be applied to classes, got '
        '`${element.name}`.',
        element: element,
      );
    }

    final className = element.name;
    final ctor = element.unnamedConstructor;
    if (ctor == null) {
      throw InvalidGenerationSourceError(
        '@CratestackBuilder() class $className must have an unnamed (default) constructor.',
        element: element,
      );
    }

    final fields = <_BuilderField>[
      for (final param in ctor.formalParameters)
        if (param.isNamed && param.isInitializingFormal) _BuilderField.from(param),
    ];

    final b = StringBuffer();
    b.writeln('class ${className}Builder {');

    // Backing fields.
    for (final f in fields) {
      b.writeln('  ${f.backingType} _${f.identifier};');
      if (f.builderRequired) {
        b.writeln('  bool _${f.identifier}Set = false;');
      }
    }
    b.writeln();

    // Fluent setters (+ add<Field> for lists).
    for (final f in fields) {
      b.writeln('  ${className}Builder ${f.setterName}(${f.dartTypeString} value) {');
      b.writeln('    _${f.identifier} = value;');
      if (f.builderRequired) {
        b.writeln('    _${f.identifier}Set = true;');
      }
      b.writeln('    return this;');
      b.writeln('  }');
      b.writeln();

      if (f.isList) {
        // Must COPY, not mutate in place: the backing field may already
        // hold a non-growable list (e.g. one handed in via the bulk setter
        // from `fromWire`'s `.toList(growable: false)`), and mutating that
        // in place with `.add` throws. Rebuilding a fresh growable list
        // via a spread on every append is the only correct shape here.
        b.writeln('  ${className}Builder ${f.addSetterName}(${f.listElemType} value) {');
        b.writeln(
          '    (_${f.identifier} = <${f.listElemType}>[...?_${f.identifier}]).add(value);',
        );
        b.writeln('    return this;');
        b.writeln('  }');
        b.writeln();
      }
    }

    // build().
    b.writeln('  $className build() {');
    b.writeln('    return $className(');
    for (final f in fields) {
      final String valueExpr;
      if (f.builderRequired) {
        final inner = f.castNeeded ? '(_${f.identifier} as ${f.dartTypeString})' : '_${f.identifier}';
        valueExpr =
            "_${f.identifier}Set ? $inner : (throw StateError('$className.${f.identifier} is required but was not set'))";
      } else if (f.listNeedsDefault(listDefaults)) {
        valueExpr = '_${f.identifier} ?? <${f.listElemType}>[]';
      } else {
        valueExpr = '_${f.identifier}';
      }
      b.writeln('      ${f.identifier}: $valueExpr,');
    }
    b.writeln('    );');
    b.writeln('  }');
    b.writeln('}');

    return b.toString();
  }
}

/// One constructor parameter's worth of builder-codegen decisions — the
/// Dart-source-derived analogue of `cratestack-client-dart::FieldView`.
class _BuilderField {
  _BuilderField({
    required this.identifier,
    required this.dartTypeString,
    required this.isNullable,
    required this.isList,
    required this.isRequiredParam,
    required this.listElemType,
  });

  factory _BuilderField.from(FormalParameterElement param) {
    final DartType type = param.type;
    final isList = type.isDartCoreList;
    String listElemType = '';
    if (isList && type is InterfaceType && type.typeArguments.isNotEmpty) {
      listElemType = type.typeArguments.first.getDisplayString();
    }
    return _BuilderField(
      identifier: param.name ?? '',
      dartTypeString: type.getDisplayString(),
      isNullable: type.nullabilitySuffix == NullabilitySuffix.question,
      isList: isList,
      // `isRequiredNamed` recovered straight from the analyzer's element
      // model — never hardcoded, never threaded through the annotation.
      // This is the load-bearing property that lets the Rust generator stop
      // emitting builders at all: required-ness is fully recoverable from
      // the Dart source of the already-generated data class.
      isRequiredParam: param.isRequiredNamed,
      listElemType: listElemType,
    );
  }

  final String identifier;
  final String dartTypeString;
  final bool isNullable;
  final bool isList;
  final bool isRequiredParam;
  final String listElemType;

  /// Mirrors `FieldView::builder_setter`: the one reserved collision is a
  /// field literally named `build`, which would shadow the builder's own
  /// terminal `build()` method.
  String get setterName => identifier == 'build' ? 'setBuild' : identifier;

  String get addSetterName =>
      'add${identifier.isEmpty ? identifier : identifier[0].toUpperCase() + identifier.substring(1)}';

  /// Mirrors `FieldView::builder_backing_type`: always a nullable spelling,
  /// even when `dartTypeString` is already nullable (required `Object?`
  /// fields).
  String get backingType => isNullable ? dartTypeString : '$dartTypeString?';

  /// Mirrors `FieldView::builder_cast_needed`.
  bool get castNeeded => backingType != dartTypeString;

  /// Mirrors `FieldView::builder_required` = `required && !is_list`. This
  /// is the key recoverability result: `isRequiredParam` alone is NOT
  /// enough (a required `List<String>` constructor param must still build
  /// as `[]`, matching issue #661) — the generator also has to inspect the
  /// parameter's own static type to know it's a list.
  bool get builderRequired => isRequiredParam && !isList;

  /// Mirrors `FieldView::list_needs_default`.
  /// Mirrors Rust's `list_needs_default = is_list && !is_patch`
  /// (`crates/cratestack-client-dart/src/field_view.rs`). Keyed off the
  /// annotation, NOT off nullability: patch and projection-model list
  /// fields are indistinguishable in the emitted Dart.
  bool listNeedsDefault(bool listDefaults) => isList && listDefaults;
}
