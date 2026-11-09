# Cursor (keyset) pagination (cratestack#1304)

Offset paging over an append-only table skips or repeats rows when inserts land
between page fetches. This adds keyset paging next to it, additively: nothing
about `limit`/`offset`, `Page<T>` or `PageInput` changes.

## Surface

| Where | Input | Output |
| --- | --- | --- |
| Procedure | `CursorPageInput` (`limit`, `cursor`) | `CursorPage<T>` (`items`, `nextCursor`) |
| Generated `list` route (REST `?cursor=`, RPC `RpcListInput.cursor`) | `cursor` + existing `limit`/`sort`/filters | `@@paged` models: `Page` whose `pageInfo.nextCursor` is set |

* `cratestack_core::{CursorPageInput, CursorPage<T>}` sit beside `PageInput`/`Page<T>`;
  `CursorPageInput::resolve(max)` applies `MAX_LIST_LIMIT`-style clamping,
  `CursorPage::from_rows(rows, limit, cursor_of)` turns a `LIMIT limit+1` fetch
  into a page. `.cstack` accepts `CursorPageInput` as a procedure argument and
  `CursorPage<T>` as a procedure return (mirroring `PageInput`/`Page<T>`).
* List routes: `cursor=` **opts in**. An empty value (`?cursor=`) is the first
  page; a previous `pageInfo.nextCursor` continues. `cursor` and `offset` are
  mutually exclusive (400). `nextCursor` is omitted from the wire unless set, so
  offset responses are byte-identical. `totalCount` (still `@@paged`) is the
  full filtered count, not the remainder.
* Cursor mode requires a `@@paged` model: a non-paged list returns a bare array
  with nowhere to carry `nextCursor`, and changing that shape would break typed
  clients. A cursor on a non-paged model is a 400.
* `MAX_LIST_LIMIT` applies unchanged to `limit`; the server fetches `limit + 1`
  internally to learn whether another page exists.

## Cursor encoding and trust

`base64url(no padding)` of JSON `{"o": "<effective order>", "v": [<value|null>...]}`:
the canonical sort the cursor was minted for (`-createdAt,id`) and the last row's
value per ordering column, primary key last, each as the wire string a
`?col__gt=` filter would carry.

**Not signed.** The cursor is untrusted input, bounded to 2048 chars, and the
server grants it nothing a caller could not already do with a filter: its values
are fed through the same typed per-field filter parsers as `?col__gt=v`, and the
resulting predicate is `AND`ed into the query beside the read policy and the
request's filters. It therefore cannot widen visibility (policy scoping is added
by the backend to every `find_many`, cursor or not), and a forged cursor only
changes where in the caller's own visible result the page starts. Binding to the
order (`o`) catches the honest mistake of reusing a cursor after changing
`sort`. It does **not** bind filters: changing filters between pages gives
pages of the new filter positioned by the old key, like any keyset API.

## Predicate

Effective order = `sort` terms + the primary key ascending as a unique
tiebreaker (no `sort` => `id`). For columns `c0..cn` with last-row values
`v0..vn` the bound is

`OR_i ( c0 = v0 AND ... AND c(i-1) = v(i-1) AND c_i AFTER v_i )`

where AFTER is `>` for ASC, `<` for DESC (`cratestack_sql::keyset_after`).
Orders use the framework's `NULLS LAST` for both directions, so on a nullable
column a non-null `v_i` is followed by `c_i IS NULL` as well, a `NULL` `v_i`
contributes no strict-after term, and equality on `NULL` is `IS NULL`.
Unsupported in cursor mode (400 `invalid_argument`): relation-path sorts
(`author.name`), repeated sort fields, non-comparable columns, composite primary
keys.

## Errors

Malformed (not base64url/JSON, wrong arity, oversized) or issued for a different
effective order: `CratestackError::BadRequest` -> REST 400, RPC
`invalid_argument`.

## RPC

`RpcListInput` gains `cursor: Option<String>`; `synthesize_list_query` pushes
`cursor=`, so RPC re-enters the REST parsing/validation path (one
implementation). Rust, Dart and TS clients carry `cursor` on list queries and
`nextCursor` on `PageInfo`; Dart/TS also get `CursorPageInput`/`CursorPage<T>`.
MCP is exempt (ADR 0002 D5).

## Backends

* Postgres (`include_server_schema!`): the generated list builder adds the
  primary-key tiebreaker order, the keyset filter and `LIMIT limit+1`.
* Embedded (`include_embedded_schema!`): has no generated HTTP surface, so no
  route/param. It gets the same primitives (`CursorPage*`, `parse_keyset_sort`,
  `keyset_after`, cursor codec) re-exported from `cratestack-sqlite`; the bound is
  an ordinary `FilterExpr`, so `find_many().where_expr(..)` on rusqlite pages
  stably. Embedded has no policy enforcement, as everywhere else.
