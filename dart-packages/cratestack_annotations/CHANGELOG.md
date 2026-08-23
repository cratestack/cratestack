# Changelog

## 0.8.7 (2026-08-23)

### `CratestackBuilder` gained `touchFlagFields` and `nonDefaultingListFields`

Both are additive, defaulting to an empty `Set<String>`, so no existing `@CratestackBuilder(...)` call
site needs to change. `package:cratestack_builder` 0.8.7 reads them to replace a by-name heuristic that
collided with ordinary schema fields (`touchFlagFields`) and to stop defaulting an unset to-many relation
field on a generated model class to `[]` (`nonDefaultingListFields`) — see that package's own CHANGELOG
for the full rationale.

## 0.8.5

Initial release. Provides `@CratestackBuilder`, consumed by
`package:cratestack_builder`.
