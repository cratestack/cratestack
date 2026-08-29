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
/// annotated class — with three exceptions, all threaded through the
/// annotation: `listDefaults`, `touchFlagFields`, `nonDefaultingListFields`.
///
/// `listDefaults` is load-bearing and was found the hard way: a projection
/// model's list field and a patch (`Update{Model}Input`) list field emit
/// BYTE-IDENTICAL Dart — `this.tags` plus `final List<String>? tags;` in
/// both cases. Patch-ness is therefore not recoverable from the generated
/// source at all. An earlier revision inferred it from nullability, which
/// made model builders emit `null` where Rust emits `[]` and regressed
/// cratestack#661's own committed regression test. The Rust generator knows
/// `is_patch` and is what emits this annotation, so it passes the answer in.
///
/// `touchFlagFields` and `nonDefaultingListFields` are for the same reason:
/// a `{field}`/`{field}IsSet` touch-flag pair and a to-many relation field
/// on a model class are both structurally indistinguishable from an
/// ordinary field/list field in the emitted Dart source — see each
/// parameter's own doc on `CratestackBuilder` for the full rationale.
///
/// Required-ness, by contrast, IS recoverable (`param.isRequiredNamed`) and
/// is never threaded through.
///
/// Unlike the lean_builder version, this generator does NOT hand-prepend a
/// `part of` header: it is wrapped in [PartBuilder] (see `../builder.dart`),
/// which emits the `part of '<file>.dart';` directive itself and validates
/// that the input file actually declares the matching `part` directive.
class CratestackBuilderGenerator
    extends GeneratorForAnnotation<CratestackBuilder> {
  const CratestackBuilderGenerator();

  @override
  String generateForAnnotatedElement(
    Element element,
    ConstantReader annotation,
    BuildStep buildStep,
  ) {
    // The pieces of schema knowledge the emitted Dart cannot carry.
    final listDefaults = annotation.read('listDefaults').boolValue;
    final touchFlagFields = annotation
        .read('touchFlagFields')
        .setValue
        .map((obj) => obj.toStringValue()!)
        .toSet();
    final nonDefaultingListFields = annotation
        .read('nonDefaultingListFields')
        .setValue
        .map((obj) => obj.toStringValue()!)
        .toSet();

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
        if (param.isNamed && param.isInitializingFormal)
          _BuilderField.from(
            param,
            isList: !nonDefaultingListFields.contains(param.name),
          ),
    ];

    // `cratestack-client-dart`'s own generated `Update{Model}Input` classes
    // pair a nullable field `foo` with a sibling `bool fooIsSet = false`
    // touch flag (`patch_touch.rs`) — the wire needs to distinguish
    // "untouched" from "explicitly cleared to null" for a nullable column,
    // and a single `foo` field can't carry that by itself.
    //
    // The OLD inline `model_builder_class.dart.j2` template encoded the
    // link by construction: `foo`'s own fluent setter flipped a shared
    // `_fooSet` tracking bool that also fed `fooIsSet:` in `build()`. This
    // generator instead treats every constructor parameter independently,
    // so without recovering the link explicitly, `.foo(value)` alone left
    // the sibling `_fooIsSet` backing field untouched and `build()`
    // silently computed `fooIsSet: false` — a real regression of
    // cratestack#663's "explicit clear" wire representation, caught by
    // `builder_edge_cases_patch_test.dart`'s `an explicitly-cleared
    // nullable field serializes as an explicit null`.
    //
    // `touchFlagFields` (read off the annotation above) names exactly the
    // fields that carry a synthesized `{field}IsSet` sibling: `other`'s
    // setter marks it touched too. An earlier revision of this generator
    // recovered the link structurally instead — a `bool`-typed field named
    // exactly `{other.identifier}IsSet` — which fires on any ordinary
    // user-declared field shaped that way, not just a real touch flag
    // (`cratestack-parser`'s `tests_patch_touch_flag_collisions.rs`
    // deliberately accepts a non-nullable `weight` beside an unrelated
    // `weightIsSet` field). Explicit annotation data avoids that false
    // positive entirely.
    final touchFlagIdentifierByField = <String, String>{
      for (final target in touchFlagFields) target: '${target}IsSet',
    };
    // The flags themselves get NO fluent setter. They are derived state: the
    // owning field's setter is what marks them, and `build()` defaults them
    // to `false`. Exposing `noteIsSet(bool)` alongside `note(..)` lets a
    // caller write `.note('x').noteIsSet(false)` and produce a patch that
    // claims "untouched" while carrying a value — order-dependent nonsense
    // the inline builder this replaces made unrepresentable by keeping its
    // tracking bool private.
    //
    // Derived from `touchFlagFields` rather than taking a fourth annotation
    // argument: naming `note` already tells us `noteIsSet` exists, so the
    // annotation stays as small as the schema knowledge genuinely requires.
    final suppressedSetters = touchFlagIdentifierByField.values.toSet();

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
      if (suppressedSetters.contains(f.identifier)) continue;
      b.writeln(
          '  ${className}Builder ${f.setterName}(${f.dartTypeString} value) {');
      b.writeln('    _${f.identifier} = value;');
      if (f.builderRequired) {
        b.writeln('    _${f.identifier}Set = true;');
      }
      final touchFlagIdentifier = touchFlagIdentifierByField[f.identifier];
      if (touchFlagIdentifier != null) {
        b.writeln('    _$touchFlagIdentifier = true;');
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
        b.writeln(
            '  ${className}Builder ${f.addSetterName}(${f.listElemType} value) {');
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
        final inner = f.castNeeded
            ? '(_${f.identifier} as ${f.dartTypeString})'
            : '_${f.identifier}';
        valueExpr =
            "_${f.identifier}Set ? $inner : (throw StateError('$className.${f.identifier} is required but was not set'))";
      } else if (f.listNeedsDefault(listDefaults)) {
        valueExpr = '_${f.identifier} ?? <${f.listElemType}>[]';
      } else if (f.needsDefaultValueFallback) {
        // An optional (non-`required`) named parameter whose OWN declared
        // type is non-nullable can only be optional because it carries a
        // default value (Dart wouldn't compile it otherwise) — e.g. this
        // generator's own `{field}IsSet` touch-flag parameter
        // (`cratestack-client-dart`'s `patch_touch.rs`), a `bool` with
        // `= false`. The backing field is still `bool?` (every backing
        // field is forced nullable — see `backingType`), so passing it
        // straight through without a fallback is a real
        // `argument_type_not_assignable` compile error whenever it's
        // never explicitly set. Recovering the parameter's own default via
        // `??` (rather than hardcoding a type-specific fallback) keeps
        // this general enough to cover any future optional-with-default,
        // non-nullable field, not just today's one instance of the shape.
        valueExpr = '_${f.identifier} ?? ${f.defaultValueCode}';
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
    required this.defaultValueCode,
  });

  /// [isList]: whether this field should be treated as a list for builder
  /// purposes (`add{Field}` setter, `?? []` default) — structural list-ness
  /// (`type.isDartCoreList`) ANDed with the caller's own
  /// `nonDefaultingListFields` exclusion (issue #661/#668 phase 3): a
  /// to-many relation field on a generated model class is still a `List<T>?`
  /// in the Dart type system, but must NOT get either behavior, so the
  /// caller passes `false` for those identifiers.
  factory _BuilderField.from(FormalParameterElement param,
      {required bool isList}) {
    final DartType type = param.type;
    final effectiveIsList = type.isDartCoreList && isList;
    String listElemType = '';
    if (effectiveIsList &&
        type is InterfaceType &&
        type.typeArguments.isNotEmpty) {
      listElemType = type.typeArguments.first.getDisplayString();
    }
    return _BuilderField(
      identifier: param.name ?? '',
      dartTypeString: type.getDisplayString(),
      isNullable: type.nullabilitySuffix == NullabilitySuffix.question,
      isList: effectiveIsList,
      // `isRequiredNamed` recovered straight from the analyzer's element
      // model — never hardcoded, never threaded through the annotation.
      // This is the load-bearing property that lets the Rust generator stop
      // emitting builders at all: required-ness is fully recoverable from
      // the Dart source of the already-generated data class.
      isRequiredParam: param.isRequiredNamed,
      listElemType: listElemType,
      // Also recovered straight from the analyzer — `null` when the
      // parameter has no default at all (only possible when it's
      // `required` or nullable-typed). See `needsDefaultValueFallback`.
      defaultValueCode: param.defaultValueCode,
    );
  }

  final String identifier;
  final String dartTypeString;
  final bool isNullable;
  final bool isList;
  final bool isRequiredParam;
  final String listElemType;
  final String? defaultValueCode;

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

  /// An optional (non-`required`, non-list) parameter whose declared type
  /// is non-nullable can only be optional because the constructor gives it
  /// a default value — Dart rejects an optional parameter with neither
  /// `required` nor a default nor a nullable type at compile time. The
  /// backing field is still forced nullable (`backingType`), so `build()`
  /// must fall back to the parameter's own default rather than passing the
  /// (possibly-null) backing field straight through — see `build()`'s use
  /// of `defaultValueCode`. `builderRequired`/`isList` are checked first at
  /// the call site, so this only fires for the remaining case.
  bool get needsDefaultValueFallback =>
      !isRequiredParam && !isList && !isNullable;
}
