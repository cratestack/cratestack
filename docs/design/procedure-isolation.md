# Enforcing `@isolation` on procedures

Status: **accepted for implementation** (2026-09-25). The maintainer decided
"enforce it now"; this document records the shape chosen and why. Security
fix for GHSA-r67q-4qqq-g9gm (embargoed at the time of writing).

Revised 2026-09-26 with five calls decided under the maintainer's standing
rule to choose the secure default (not decided by the maintainer directly),
each recorded where it
applies: a typed SQLSTATE is authoritative for retries (§5),
`TRANSACTION_ABORTED` is final (§5), a nested `@isolation` call joins the
outer attempt and only the dispatch that owns an exhausted attempt releases
its idempotency key (§6, §7.1), only that owner answers
`TRANSACTION_ABORTED` — any other abort is a 500 `INTERNAL_ERROR` (§6) —
and the resolver trait documents that resolvers re-run (§6).

## 1. The defect

`@isolation("serializable" | "repeatable_read" | "read_committed")` on a
procedure has been accepted since 0.2.0. The parser validates the level
(`cratestack-parser/src/validate/procedures.rs`) and
`cratestack_core::TransactionIsolation` parses it, but no macro ever read
it. Dispatch (`procedure/instrument/invoke_with_db.rs`) handed the
`ProcedureRegistry` method the ordinary pool-backed `Cratestack`, and the
`db.transaction(..)` combinator (`cratestack-sqlx/src/transaction.rs`) opens
a plain `BEGIN`. Measured on `origin/main`: a declared-serializable
procedure reports `current_setting('transaction_isolation') = 'read
committed'` on REST and RPC, and two concurrent read-check-write withdrawals
of 100 from a balance of 100 both succeed. The same code shape under a real
`SERIALIZABLE` transaction refuses one with SQLSTATE 40001.

`cratestack-sqlx/src/isolation.rs` said a macro recorded the level on a
`ProcedureMetadata` constant that did not exist; both READMEs advertised
"transaction isolation control".

## 2. What "enforced" means

A procedure that declares `@isolation(level)` runs, on every path that can
execute it, inside one Postgres transaction begun with
`BEGIN ISOLATION LEVEL <level>`. Its authorization (`@allow`/`@deny` and any
`@authorize` model check) and its body run inside that transaction. On
SQLSTATE `40001` (serialization_failure) or `40P01` (deadlock_detected) —
raised by a statement or deferred to `COMMIT` — the transaction is rolled
back and the authorization plus body run again, up to a retry budget. All
database access the body can make **through the handle it is given** goes
through that transaction.

The paths: REST (`/$procs/<name>`), RPC unary (`/rpc/procedure.<name>`),
RPC batch (`/rpc/batch`, each op dispatched through the same per-procedure
handler), MCP `tools/call`, and non-HTTP callers of the generated
`<procedure>::invoke_with_db`. All five go through `invoke_with_db`, so the
wrapper lives there, once.

## 3. The handle: a distinct `IsolatedCratestack` type

### 3.1 Options considered

1. **Keep `db: &Cratestack`, make it transaction-bound at runtime.** The
   same type is pool-backed for every other procedure, so `db.pool()` stays
   callable. It can only be made to fail at runtime (a closed pool, a
   panic), and `db.views()`, `db.queries()` and `db.events().drain()` would
   each need their own runtime refusal. An escape that compiles is the bug
   class being fixed.
2. **Hand the body `&mut Tx` only.** No escape, but the body loses the
   model accessors, and write builders need a `&Cratestack` to exist at all;
   passing both reintroduces option 1.
3. **A distinct generated type whose every method is transaction-bound.**
   Chosen.

### 3.2 `IsolatedCratestack`

Generated next to `Cratestack` for `db = Postgres` schemas that declare at
least one `@isolation` procedure (no new type otherwise; the only addition
to a schema without `@isolation` is `CratestackBuilder::with_isolation_max_retries`,
which every `db = Postgres` schema gets). The `ProcedureRegistry` method of an
`@isolation` procedure takes `db: &super::IsolatedCratestack` instead of
`db: &super::Cratestack`. Every other method's signature is unchanged.

Its surface is exactly what can be routed through the transaction:

| Method | Behaviour |
| --- | --- |
| model accessors (`db.account()` …) | the usual `ModelDelegate`s, over a transaction-bound runtime (§4) |
| `bind_context` / `bind_auth` | the usual `BoundCratestack`, over the same bound runtime |
| `transaction(async \|tx\| ..)` | a `SAVEPOINT` inside the isolated transaction (§7); `tx` is the raw-SQL door |
| `dispatch_audit_sink(events)` | deferred until the isolated transaction commits (§6) |

Deliberately absent: `pool()` (the escape hatch), `events()` (its `drain()`
writes delivery marks on the pool), `views()` and `queries()` (both execute
on the pool; routing them is follow-up work, and until then their absence
is a compile error rather than a silent escape). A procedure that needs raw
SQL uses `db.transaction(async |tx| sqlx::query(..).execute(&mut ***tx))`.

It is not `Clone`, and it has only private fields, so it cannot be
constructed outside the generated `cratestack_schema` module: a caller
cannot build one over the pool and call the registry method directly. The
generated dispatch hands it to the caller's closure *by value* (see §5):
a closure over a borrowed handle needs a higher-ranked `Send` bound the
compiler cannot prove for an axum handler's future (measured: "implementation
of `Send` is not general enough"). A handle kept past the procedure's end
is inert: its transaction has been taken for commit or rollback, and every
operation returns `INTERNAL_ERROR`.

## 4. The transaction-bound runtime

`SqlxRuntime` (`cratestack-sqlx`) gains an optional bound transaction:
`Option<Arc<BoundTx>>`, where `BoundTx` holds the live
`sqlx::Transaction<'static, Postgres>` behind a `tokio::sync::Mutex`, the
deferred `AuditEvent`s, a "drain the outbox after commit" flag, and a
"this attempt saw a retriable error" flag. A pool-backed runtime has `None`
and every existing code path is unchanged for it.

On a bound runtime:

- every builder's `run()` (reads, writes, aggregates, projections,
  includes, batch operations) opens a savepoint on the bound transaction and
  runs its existing `run_in_tx` body inside it — one code path per
  operation, the one already used by `db.transaction(..)` callers;
- `authorize_detail`/`authorize_update`/`authorize_delete` (what
  `@authorize` expands to) read through the bound transaction;
- `transaction(..)` opens a savepoint instead of a new pool transaction.

Each operation gets its own savepoint so a failed statement does not abort
the whole isolated transaction: a body that catches, say, a unique
violation from one `create` and carries on behaves exactly as it did on
the pool. (The cost is one `SAVEPOINT`/`RELEASE` round trip pair per
operation.)

### 4.1 Policy reads inside an attempt run on the attempt's transaction

Several `run_in_tx` paths evaluate policies on the pool rather than on the
connection the write runs on: create policies (including their relation
`EXISTS` lookups), the update/delete `@version` probe that tells a 412 from
a 403, and the upsert update-policy checks. For an isolated procedure that
would be an isolation hole (a policy decision made on data outside the
snapshot) and a pool-starvation deadlock (each in-flight procedure holds
one connection and waits for a second; with `N` concurrent procedures on
an `N`-connection pool nothing progresses until `acquire_timeout`).

So on a runtime **bound to an `@isolation` attempt**, those reads use the
attempt's transaction (`PolicyDb::of` in `cratestack-sqlx`): they see the
attempt's own earlier writes, read its snapshot, and never take a second
connection. This covers `.run()` through the handle, `run_in_tx(tx, ..)`
inside the handle's `transaction(..)`, and the `batch_*` builders called
through the handle. `tests/procedure_isolation_policy.rs` pins it.

**Every other caller is unchanged** (maintainer decision): a
`db.transaction(..)`, `run_in_tx`, `run_in_isolated_tx`, `batch_*` or
audited/emitting `.run()` on a pool-backed runtime reads policies on the
pool exactly as before this change, and `tests/policy_db_caller_tx.rs`
passes unchanged on the parent commit. That keeps three pre-existing
limitations for those callers, recorded here rather than changed:

- the probe does not see the caller's own uncommitted writes (a parent
  created earlier in the same transaction does not authorise its child; a
  parent handed to another owner earlier still does; a later `batch_create`
  item is not authorised by an earlier one);
- under `REPEATABLE READ`/`SERIALIZABLE` the probe reads the latest
  committed data, not the caller's snapshot;
- the probe needs a second pooled connection while the caller's
  transaction holds one. With `N` concurrent such writers on an
  `N`-connection pool — including audited or emitting `.run()` calls whose
  create policy has a relation lookup, since their framework transaction
  holds a connection while the probe asks for another — nothing progresses
  until `acquire_timeout`.

The one-time `cratestack_audit` bootstrap follows the same split. Inside an
attempt it first asks the attempt's transaction whether the table and the
three indexes `AUDIT_TABLE_DDL` creates exist (`to_regclass`) and takes a
pool connection for the DDL only when one is missing, so a table created
without its indexes still gets them. Everywhere else it runs the DDL on the
pool once per runtime, as before.

### 4.2 One statement at a time

A Postgres connection runs one statement at a time. The handle's
operations take the transaction's lock with `try_lock`: concurrent use
(`tokio::join!` of two operations on one handle) or re-entrant use (calling
`db.account().create(..).run(ctx)` *inside* `db.transaction(async |tx| ..)`,
where `tx` already holds it) fails immediately with
`CratestackError::Internal` rather than deadlocking. Inside a
`transaction` closure, pass `tx` to `run_in_tx(tx, ctx)` instead.

## 5. Retries

The body must be re-runnable. The generated `invoke_with_db` re-runs
authorization and the registry method on each attempt: it takes a
`FnOnce(IsolatedCratestack, Authorized) -> Fut + Clone` closure and clones
it per attempt, so the generated dispatch's closure, which owns a clone of
`Args`, the context and the registry, hands the method fresh values each
time.

- Retriable: SQLSTATE `40001` or `40P01`, from any statement or from
  `COMMIT`. Detection is `retriable_sqlstate` (`retriable.rs`). Only
  database errors: an application or validation error whose message
  happens to contain `40001` (`field 'memo' length 40001 exceeds maximum
  100`) is not a serialization failure, and treating it as one re-ran the
  body up to the budget and answered 409.
- **A typed SQLSTATE is authoritative** (decided 2026-09-26 under the maintainer's standing secure-default rule).
  A database error that carries a SQLSTATE (`DatabaseTyped`, `ConflictTyped`)
  is retriable if and only if that SQLSTATE is `40001` or `40P01`; its text
  is never consulted, and a typed error without a code is not retriable.
  Text matching remains only for the untyped `Database(String)` variant,
  which has no SQLSTATE to read (sqlx errors other than `Database`, and
  errors built by hand). Reason: the previous classifier fell back to the
  text when the SQLSTATE was not a retriable one, and Postgres echoes
  request data into the messages of other SQLSTATEs. A body's `RAISE
  EXCEPTION 'insufficient funds: requested %'` with `40001` from the request
  (`P0001`), or a cast of the request's `"40001x"` to `bigint` (`22P02`,
  `invalid input syntax for type bigint: "40001x"`), re-ran the body up to
  the budget, answered `409 TRANSACTION_ABORTED` instead of the real error,
  and released the `Idempotency-Key`: request-triggerable. The unit test
  that pinned the fallback (an unknown `XX999` whose detail says `could not
  serialize access` was retried) now pins the opposite, and
  `procedure_isolation_nested.rs` drives both echoes through an `@isolation`
  procedure (the body runs once, the real 500 is returned, and it is
  recorded under its key).
  The rule first reached only errors built with `cratestack_error_from_sqlx`.
  The framework's own reads — `find_unique`, `find_many`, the projected
  reads, the aggregates — its `@authorize` probe, its create-policy relation
  lookup and its `@@audit` writes turned every sqlx error, a Postgres one
  included, into the untyped `Database(error.to_string())`, and so were
  classified by their text: a model over a view that casts a stored
  `'40001x'` to `bigint` made `find_unique` fail with `22P02`, and the body
  ran four times per request, answered `409 TRANSACTION_ABORTED` and
  released the key (measured, `procedure_isolation_nested.rs`). They now
  use `cratestack_error_from_sqlx` too; their errors keep the same code,
  status and public message (`DATABASE_ERROR`, 500), gain `db_sqlstate()`,
  and a `RowNotFound` cannot occur on those paths (`fetch_optional`, or a
  `fetch_one` of an aggregate or `EXISTS` that always returns a row).
- **`TRANSACTION_ABORTED` is final** (decided 2026-09-26 under the maintainer's standing secure-default rule). The
  error an exhausted loop returns keeps its `40001`/`40P01` in `sqlstate`
  (`db_sqlstate()` still reports it), but the classifier never treats it as
  retriable. Reason: it is the outcome of a retry loop, not a statement's
  failure. When an `@isolation` body propagated another transaction's
  exhausted abort (a procedure it called through a pool handle it holds
  itself, §11), the outer loop classified it as a serialization failure and
  ran the whole body — the inner loop included — up to the budget again.
  The outer attempt now fails once with it (`procedure_isolation_nested.rs`).
- **The hand-rolled `run_in_isolated_tx` uses the same classifier.** Its
  older one also text-matched `40001`/`40P01` and the Postgres phrases
  inside every error variant. This was first kept unchanged, so as not to
  alter code that does not use `@isolation`, and then aligned (decided
  under the maintainer's standing secure-default rule): an application or validation error whose text contains
  `40001` (a body echoing request data) being retried re-runs the body with
  whatever it did outside the transaction, up to four times, and returns
  the last attempt's error instead of the first: a correctness and safety
  bug on any path, not a behaviour preference. The maintainer's decision to
  split the helper out of this fix covered its *policy reads* (§4.1), not
  its classifier. A typed `40001`/`40P01` and an untyped `Database` whose
  text says so are still retried exactly as before; the non-database
  variants, and typed database errors of any other SQLSTATE whose text
  happens to contain `40001` or a Postgres phrase, stop being retried
  (`banking_isolation`'s
  `an_application_error_mentioning_40001_is_returned_once_not_retried`).
- **Tainted attempts.** If any operation through the handle observed a
  retriable error during an attempt, the attempt is rolled back and retried
  even when the body swallowed or re-wrapped the error (for example mapped
  it to its own `Internal`). The commit of such an attempt is never tried.
- Budget: 3 retries (4 attempts), the existing `MAX_RETRIES_DEFAULT`.
  Configurable per runtime with
  `CratestackBuilder::with_isolation_max_retries(n)`; `0` means no retry.
- Backoff before retry `k` (1-based): `2ms × 2^(k-1)`, capped at 64 ms,
  plus up to the same amount again of jitter from the clock's sub-second
  nanoseconds. No transaction is held while waiting.
- Exhausted: `CratestackError::TransactionAborted(TransactionAbort)`, whose
  `info` has `sqlstate` `40001` (or `40P01`) and the fixed public detail
  `transaction could not be completed because of concurrent updates; retry
  the request`, and whose `ownership` is `Exhausted`; the generated
  dispatch that owns the loop claims it (§6) and answers HTTP
  **409**, code **`TRANSACTION_ABORTED`**, RPC code **`aborted`**
  (gRPC's `ABORTED`, "typically due to a concurrency issue such as a
  transaction abort"). 409 rather than 500 because the request was correct
  and a retry is expected to succeed; rather than 503 because the
  framework already answers database-origin conflicts with 409. Its own
  code rather than `CONFLICT` (maintainer decision) because the two mean
  opposite things to a client: a unique violation's `CONFLICT` is repeated
  by a retry, an aborted transaction is not, and a client must not have to
  match on the message to tell them apart. The driver's message is never
  exposed. Every generated client knows the code (Rust maps `aborted` to
  409 for batch frames, the TypeScript and Dart RPC runtimes list it, as
  does `@cratestack/link-batch`). A hand-rolled `run_in_isolated_tx` that
  runs out of retries still returns the last error it saw (a `40001` is a
  `DatabaseTyped`, 500); the two paths disagree, and aligning them is a
  separate decision.

## 6. Side effects

| Effect | Where | On retry / rollback |
| --- | --- | --- |
| model writes, `@@audit` rows, `@@emit` outbox rows | inside the isolated transaction | rolled back with the attempt |
| `AuditSink` fan-out (automatic after `run()`, or explicit `dispatch_audit_sink`) | deferred; dispatched once, after the committed attempt | discarded |
| outbox drain (automatic after an emitting `run()`) | deferred; once, after commit | not performed |
| `@computed` output composition | inside the attempt, after the body, before `COMMIT` | repeated with the attempt; a resolver error fails the attempt |
| `Idempotency-Key` reservation and completion | the tower layer around the whole handler (HTTP), `admit_and_run` (MCP) | once per request; retries are invisible to it |
| rate-limit admission | the tower layer / MCP admission, before dispatch | once per request |
| anything the body does outside the database (HTTP calls, e-mail, a counter in `self`) | the body | **repeated**; the body must be idempotent or push the effect through `@@emit` |

**`@computed` fields** (maintainer decision). For an `@isolation`
procedure whose output reaches a `@computed` field, the generated dispatch
(REST, RPC, `/rpc/batch` and MCP alike) composes the output inside the
attempt closure, after the registry method returns and before `invoke_with_db`
commits. The resolvers receive `&Cratestack` as always — the
`ComputedFieldResolver` trait is unchanged — but it is the `Cratestack`
inside the attempt's `IsolatedCratestack`, over the attempt's transaction:
a resolver's model reads see the body's writes and its snapshot. A resolver
error fails the attempt, which is rolled back; it is retried only if it is
a retriable database error (or the attempt was tainted), like any body
error. Procedures without `@isolation`, and `invoke_with_db` called by hand
(which never composes), are unchanged: composition runs after the call, on
the pool. The resolver's `&Cratestack` still has `pool()`, `views()`,
`queries()` and `events()`, which run on the pool (§11). A resolver that
calls another `@isolation` procedure's `invoke_with_db` with that
`&Cratestack` joins the attempt (§7.1).

Resolvers therefore re-run with every retried attempt and must be
re-runnable, like the body. Because the trait's signature did not change,
nothing forces an implementor to notice, so the generated
`ComputedFieldResolver` trait of a schema that declares an `@isolation`
procedure carries a doc comment saying so, and each `@isolation`
procedure's `ProcedureRegistry` method's doc says its output's resolvers run
inside the same attempt (decided 2026-09-26 under the maintainer's standing secure-default rule). A schema without
`@isolation` gets no new doc attribute.

**Idempotency** (maintainer decision). A `TRANSACTION_ABORTED` outcome of
the dispatched procedure's own attempt is not recorded: nothing was
committed, so the same `Idempotency-Key` must be able to run the call again
rather than replay the 409.

*Only the owner releases* (decided 2026-09-26 under the maintainer's standing secure-default rule). "Nothing was
committed" is true of the transaction whose retries ran out, not of every
caller the error travels through. A procedure without `@isolation` that
commits a debit on the pool and then propagates another procedure's
`TRANSACTION_ABORTED` with `?` has committed work; releasing its key let the
same key debit again. So `TransactionAbort` carries an `AbortOwnership`:

| State | Set by | Answered as | Recorded under a key? |
| --- | --- | --- | --- |
| `Exhausted` | `run_isolated` when its own attempts ran out | 500 `INTERNAL_ERROR` | yes |
| `Propagated` | `run_isolated` (and a joined call, §7.1) on any error a body returned | 500 `INTERNAL_ERROR` | yes |
| `Claimed` | the generated REST/RPC/MCP dispatch of an `@isolation` procedure, on its `invoke_with_db` result (`__generated_claim_transaction_abort`: `Exhausted` → `Claimed`) | 409 `TRANSACTION_ABORTED` | **no** |

`CratestackError::is_idempotency_replayable` is `false` for a `Claimed`
abort and only for it. The release is opt-in at the one place that knows
it owns the attempt: a non-`@isolation` dispatch never claims, and a
hand-written caller of `invoke_with_db` receives `Exhausted`. An
`@isolation` procedure that propagates another transaction's abort does not
own it either: its own retries did not run out.

*`Claimed` cannot be forged* (decided 2026-09-26 under the maintainer's standing secure-default rule). Since
`Claimed` releases a key, the table's "set by" column has to be a property
of the type, not a convention. In the first cut it was not: `ownership` was
a public field and `TransactionAbort::new(info, ownership)` took any state,
so a procedure body, a `@computed` resolver or a hand-written handler could
return a `Claimed` abort after committing work, which had its key released
and was answered as `TRANSACTION_ABORTED`: exactly the double debit this
section exists to prevent. So:

- `ownership` is private; `TransactionAbort::ownership()` reads it.
- The public constructors take no ownership: `TransactionAbort::exhausted`
  and `TransactionAbort::propagated` build the two recorded states, and
  `Default` is `Exhausted`. `new(info, ownership)` is gone (the type is new
  in this release, so nothing released depended on it).
- The one way to `Claimed` is the `#[doc(hidden)]`
  `CratestackError::__generated_claim_transaction_abort` (formerly
  `claim_transaction_abort`), whose name states the contract. Only the
  generated code calls it: `axum/procedure/invoke_call.rs` (REST and RPC)
  and `include/server/mcp_module/isolated_arm.rs` (MCP). The struct is
  `#[non_exhaustive]` as well, so a struct literal fails twice over.
- `cratestack-core`'s `error/aborted_doctests.rs` holds `compile_fail`
  doctests for assigning the field, a struct literal, struct-update syntax
  and a constructor that takes an ownership, each next to a compiling twin
  that differs by that one line.

This is a boundary against forging, not against calling. The claim function
is public because generated code lives in the consuming crate, so
hand-written code can still call it, on an abort it built with
`TransactionAbort::exhausted` or one its own `invoke_with_db` returned (it
turns only `Exhausted` into `Claimed`; a `Propagated` abort stays
`Propagated`, and an `@isolation` retry loop re-marks anything its body
returns as `Propagated`). What that can do is bounded by where the error
goes: a response, and so the key of the request the calling code is
answering. A handler, body or resolver that does it after committing work
of its own releases its own request's key and tells its own client to
retry; it cannot release another request's key. That is a documented contract (the function's doc says "generated
code only"), not a security boundary, in the same way that a handler that
returns a success it did not earn is the handler's own bug.

*Only the owner answers `TRANSACTION_ABORTED`* (decided 2026-09-26 under the maintainer's standing secure-default rule). The code tells a client that nothing was committed and to send
the request again. A non-owner cannot say that: a procedure that committed
a debit and then propagated another procedure's abort, answered with it,
invites a retry under a new key that debits twice; recording the response
under the old key does not help a client that was told to retry. So every
abort that is not `Claimed` is answered as a 500 `INTERNAL_ERROR` (RPC
`internal`), whose operator detail keeps the original SQLSTATE and message
(`CratestackError::disowned_transaction_abort`, logged at `warn`), and is
recorded like any other 500. The substitution is made where an error
becomes a response, not in each generated dispatch: the REST/RPC encoders
(`cratestack-axum`'s `idempotency::answered`, used by the unary, sequence
and codec encoders and by `encode_rpc_error`; RPC unary and `/rpc/batch`
re-enter them) and MCP's tool-call (`admission/run.rs`) and resource-read
(`resources/error.rs`) error paths. That covers, with one rule, a
non-`@isolation` procedure, an `@isolation` one that propagates, a model
route whose `@computed` resolver propagates one, and a hand-written handler
that returns an unclaimed abort from `invoke_with_db` — which therefore
answers 500 and is recorded rather than 409 and released; serving the
procedure through the generated router is what makes the owner's answer
available. Doing it at the encoders also keeps the generated code of a
procedure without `@isolation` unchanged (§9).
`procedure_isolation_nested.rs` pins both non-owner cases (500, recorded,
replayed, debited once) next to the owner's released 409;
`tests_unrecorded.rs`, `tests_aborted.rs`, MCP's `admission.rs` and
`resources/tests_error.rs` pin the encoders.

On HTTP the encoders that turn a handler error into a response tag the
response with a response extension (never sent on the wire), RPC's error
re-encoding carries the tag, and `IdempotencyLayer` releases the
reservation instead of completing it. MCP's `admit_and_run` checks the
error directly and releases the reservation through the same `OpExecutor`,
under the namespace the reservation was taken under: since cratestack#1033
that is `mcp:<sha256 hex of the principal id>` (`mcp-system:` for a system
caller), and `cratestack-mcp/tests/admission_abort.rs` and
`procedure_isolation_mcp.rs` pin that the release and the recorded row use
it.
Every other response is recorded as before, errors included.
`/rpc/batch` refuses an `Idempotency-Key` header outright, so no batch
response is ever recorded under one.

## 7. Nested `db.transaction(..)` inside an isolated procedure

A savepoint: `SAVEPOINT cratestack_isolated_nested`, `RELEASE` on `Ok`,
`ROLLBACK TO` on `Err`. The closure's `tx` is the isolated transaction
itself, so `run_in_tx(tx, ctx)` inside it is covered by the same isolation.
A retriable error returned from the closure still taints the attempt.

`tx` is raw, so the closure can leave the transaction *aborted* (a
statement fails and the closure does not return the error) or *end* it
(`COMMIT`, `ROLLBACK`). Either way the `RELEASE` (on `Ok`) or the
`ROLLBACK TO` (on `Err`) of the savepoint fails. That failure **poisons
the attempt**: it is rolled back and fails with `INTERNAL_ERROR`, whatever
the body returns afterwards. Without it, a body that also ignored
`transaction`'s error returned `Ok`, and Postgres answers `COMMIT` on an
aborted transaction with a silent `ROLLBACK`: measured, `200
{"before":100,"after":0}` for a debit that never happened
(`procedure_isolation_escape.rs`). A raw `COMMIT` is detected the same way,
but what ran before it is already committed and cannot be undone: the
caller gets an error for work that is partly durable. That is raw SQL in
trusted code, the same trust boundary as any other raw statement.
`SET TRANSACTION ISOLATION LEVEL` through `tx` is refused by Postgres
(inside a savepoint), so the level cannot be lowered.

A `transaction(..)` future **dropped before it finished** — cancelled by
the body's own timeout or `select!`, or unwound by a panic — also poisons
the attempt (a drop guard in `bound/nested.rs`). Its savepoint is then
still open with whatever the closure wrote, and nothing will roll back to
it; on the pool, sqlx's `Transaction` guard rolls a dropped transaction
back, but this savepoint is raw SQL with no such guard. Measured before the
guard: a body that timed out its own `transaction(..)` after a credit and
returned `Ok` answered `200` and committed the credit
(`procedure_isolation_nested.rs`). A joined `@isolation` call has the same
guard (§7.1).

**Raw `COMMIT` after a swallowed serialization failure** (a documented
trust boundary, decided 2026-09-26 under the maintainer's standing secure-default rule; not changed). A poisoned
attempt that is *also* tainted — an operation earlier in it saw a
`40001`/`40P01` that the body swallowed — is classified by the taint and
retried, not failed: a tainted attempt is always retried (§5). If the
poison came from a raw `COMMIT` through `tx`, the writes made before that
`COMMIT` are already durable, and the retried attempt makes them again, so
they are committed twice, and the idempotency key is released if the
retries then run out. The framework cannot undo a `COMMIT` that trusted
code issued on the raw connection; making such an attempt fail instead of
retrying would still leave the partly durable work, and would change the
retry contract for every tainted attempt. Raw transaction control through
`tx` is therefore outside the guarantee, like a pool held in `self` (§11):
never issue `COMMIT`, `ROLLBACK` or `END` through it.

### 7.1 A nested `@isolation` call joins the outer attempt

Decided 2026-09-26 under the maintainer's standing secure-default rule. A `@computed` resolver of an `@isolation`
procedure's output receives a `&Cratestack` bound to the attempt (§6), and
can pass it to another `@isolation` procedure's `invoke_with_db`. That
call's `run_isolated` then ran on a runtime already bound to an attempt,
ignored the binding, and began a second transaction on the pool: the inner
procedure committed on its own, before the outer attempt did. An outer
rollback kept the inner writes; every outer retry committed them again; and
the inner call needed a second pooled connection while the outer held one.

`run_isolated` on a bound runtime (`bound/join.rs`) now:

- compares the inner procedure's declared level with the attempt's. A
  stricter inner level is **refused** before anything runs, with
  `INTERNAL_ERROR` (operator detail: a transaction's isolation level cannot
  be raised after it has begun). Postgres cannot raise the level of a
  running transaction, and running the inner body at a weaker level than it
  declared is the defect this fix closes. An equal or weaker declared level
  runs at the attempt's (stricter) level;
- runs the inner authorization and body **once**, in a savepoint
  (`cratestack_isolated_join`) of the outer transaction, with the
  same bound runtime: its operations, policy reads and `transaction(..)`
  savepoints are all on the outer transaction;
- on `Ok`, releases the savepoint; on `Err`, rolls back to it (and drops
  the `AuditSink` events queued inside it, whose writes it just undid) and
  returns the error unchanged, except that a `TransactionAbort` in it is
  marked `Propagated` (§6);
- never retries and never produces `TRANSACTION_ABORTED`. A retriable error
  inside the joined call taints the outer attempt; **retries are owned by
  the outermost attempt only**, which rolls back and re-runs everything —
  the joined call included;
- poisons the outer attempt (§7) if its savepoint cannot be closed, or if
  the joined call is dropped before it finishes (cancelled by a timeout or
  `select!`, or panicking): its savepoint is then still open with part of
  its writes, and a `COMMIT` would keep them;
- runs **one at a time** per attempt: a joined call started while another
  is still running on the same attempt is refused before its `SAVEPOINT`,
  and the refusal poisons the attempt. Two concurrent joined calls
  (`tokio::join!` in a resolver) interleave their operations between the
  savepoints, so the later one's `ROLLBACK TO` undoes the earlier one's
  writes made after it began while that one still reports `Ok`. The first
  version detected this only when the earlier call finished first, by a
  counter check at its end; in the other order the attempt committed a
  debit whose joined credit had been silently undone (measured: `200`,
  balances `(90, 500)`). A call started from inside a running joined call,
  through the resolver's handle captured in its closure, cannot be told
  apart from a concurrent one (both use the attempt's root handle, and the
  joined body's own `IsolatedCratestack` cannot reach `invoke_with_db`), so
  it is refused the same way.

`procedure_isolation_nested.rs` pins it: the joined call reports the outer
transaction's id and level, a credit it makes in an attempt that is then
retried is committed once, not once per attempt, and a `serializable` call
inside a `repeatable read` attempt is refused and rolls the outer attempt
back; two joined calls running at once, and a joined call cancelled after
it wrote, both leave nothing committed.

The body of an `@isolation` procedure has no `&Cratestack` to pass (its
`IsolatedCratestack` has none, by design, §3.2), so the resolver is the
path that reaches this. An implementor that calls a procedure through a
pool-backed handle it holds itself gets an independent transaction (§11).

## 8. What is refused, and where

| Situation | Decision |
| --- | --- |
| `@isolation` on a `@stream` procedure | parse error: a streamed response is produced after the procedure returns, so there is no point at which to commit, and a partially sent stream cannot be retried |
| `@isolation` in a `datasource { provider = "none" }` schema | parse error: there is no database |
| `@isolation` in a schema compiled with `db = None` but no datasource block | `compile_error!` from `include_server_schema!` |
| `include_embedded_schema!` | unchanged: the embedded role generates no procedures at all, so there is nothing to enforce |
| `include_client_schema!` | unchanged: the attribute describes the server's behaviour; the client stub has nothing to do |


## 9. Procedures without `@isolation`

Token-identical generated code: same registry signature, same
`invoke_with_db`, same dispatch (measured with `cargo expand` on
`find_many_procedure`, `computed_fields_rpc` and `mcp_policy_pg`: the only
difference is the `CratestackBuilder::with_isolation_max_retries` method).
At runtime, every executing builder checks `SqlxRuntime::bound()` first (a
`None` on the pool-backed runtime) and then runs exactly the code, and the
policy reads, it ran before (§4.1). The changes a caller without
`@isolation` can observe are the new error variant, which it never
receives from its own procedures (it can receive, and propagate, another
procedure's; its dispatch never claims it, so the response encoders answer
it as a 500 `INTERNAL_ERROR`, recorded under a key like any other 500, §6),
and the hand-rolled `run_in_isolated_tx`'s classifier (§5): it
no longer retries a non-database error, or a typed database error of
another SQLSTATE, because of its text. The ownership rule (§6) is applied
in the dispatch of `@isolation` procedures (claiming) and in the response
encoders (answering a non-owner's abort as `INTERNAL_ERROR`), so it adds no
token to theirs.

## 10. Breaking changes

1. The `ProcedureRegistry` method of an `@isolation` procedure takes
   `db: &IsolatedCratestack`. Code that used `db.pool()` there must use
   `db.transaction(async |tx| ..)`; code that wrapped its own body in
   `run_in_isolated_tx(db.pool(), ..)` must drop the wrapper (the dispatch
   now provides it; the old code would not compile anyway).
2. `<procedure>::invoke_with_db` for an `@isolation` procedure takes
   `FnOnce(IsolatedCratestack, Authorized) -> Fut + Clone` instead of
   `FnOnce(Authorized) -> Fut`.
3. The body now runs with retries, so it must be re-runnable; so do its
   `@computed` resolvers, which now run inside the attempt (§6).
4. `CratestackError::TransactionAborted(TransactionAbort)` is a new variant
   (the enum is `#[non_exhaustive]`, so a downstream `match` already has a
   wildcard arm), with the new public types `TransactionAbort` and
   `AbortOwnership` (§6); application code builds a `TransactionAbort`
   only as `exhausted` or `propagated`, and reads its ownership through a
   getter. A client that treated every 409 as `CONFLICT`
   sees a new code.
5. An `@isolation` procedure invoked from inside another's attempt (§7.1)
   joins it instead of committing on its own, and one that declares a
   stricter level than the attempt is refused.

`run_in_isolated_tx` / `run_in_isolated_tx_with_retries` are unchanged
except for the classifier they now share with dispatch (§5): an
application or validation error whose text contains `40001`/`40P01` or the
Postgres phrases, and a typed database error whose SQLSTATE is neither
`40001` nor `40P01` whatever its text says, are returned on the first
attempt instead of being retried.

## 11. What remains outside the guarantee

- State a `ProcedureRegistry` implementor holds itself (a `PgPool` stored in
  `self`) is not the handle and can do anything. Same boundary as holding a
  raw connection to bypass row policy. A procedure called through such a
  handle runs in its own transaction, not the attempt's (§7.1 applies only
  to the attempt-bound handle); if its retries run out, the calling body
  gets a final `TRANSACTION_ABORTED` that the outer loop does not retry,
  and that the caller's response answers as `INTERNAL_ERROR`, recorded
  under its key (§5, §6).
- `views()` and `queries()` are not available inside an `@isolation`
  procedure yet.
- A `@computed` resolver of an `@isolation` procedure's output receives a
  `&Cratestack` whose model accessors and `transaction(..)` run on the
  attempt, but whose `pool()`, `views()`, `queries()` and `events()` run on
  the pool: outside the snapshot, and needing a second connection while the
  attempt holds one (on a small pool, a wait until `acquire_timeout`).
  Changing the resolver trait's handle type would close this and break every
  resolver implementation; it was not done.
- When `cratestack_audit` does not exist yet, its one-time DDL bootstrap runs
  on the pool; it creates schema, not procedure data. Where migrations
  created the table (the normal case), no second connection is taken.
- Raw SQL through `transaction()`'s `tx` can end the transaction early
  (`COMMIT`). The attempt then fails (§7), but what ran before the
  `COMMIT` stays committed; and if the attempt had also swallowed a
  serialization failure, it is retried and that work is committed again
  (§7, "Raw `COMMIT` after a swallowed serialization failure"). Trusted
  code must not issue transaction control through `tx`.
- `@isolation` does not make an RPC batch atomic: each frame is its own
  transaction, as each frame was its own call before.
- Callers without `@isolation` keep reading policies on the pool, with the
  limitations listed in §4.1, including the pool-starvation wait for audited
  writes whose create policy has a relation lookup.
