import 'package:cratestack_annotations/cratestack_annotations.dart';
import 'package:test/test.dart';

void main() {
  test('listDefaults defaults to true', () {
    // Every class kind except patch inputs wants list defaulting, so the
    // safe default is the common case — a generator that forgets to pass
    // the flag produces model-correct output rather than patch-correct
    // output, and patch classes are the ones the emitter treats specially.
    expect(const CratestackBuilder().listDefaults, isTrue);
  });

  test('listDefaults can be turned off for patch inputs', () {
    expect(const CratestackBuilder(listDefaults: false).listDefaults, isFalse);
  });

  test('is a const constructor, so it is usable as an annotation', () {
    // Not a tautology: making any field non-final or the constructor
    // non-const would make `@CratestackBuilder()` a compile error at every
    // use site, and the failure would surface in consumers rather than here.
    const annotation = CratestackBuilder(listDefaults: false);
    expect(annotation, isA<CratestackBuilder>());
  });

  test('touchFlagFields and nonDefaultingListFields default to empty', () {
    const annotation = CratestackBuilder();
    expect(annotation.touchFlagFields, isEmpty);
    expect(annotation.nonDefaultingListFields, isEmpty);
  });

  test('touchFlagFields and nonDefaultingListFields are settable', () {
    const annotation = CratestackBuilder(
      touchFlagFields: {'note'},
      nonDefaultingListFields: {'posts'},
    );
    expect(annotation.touchFlagFields, {'note'});
    expect(annotation.nonDefaultingListFields, {'posts'});
  });
}
