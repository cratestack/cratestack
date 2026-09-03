# Design: bounding the rate-limit bucket keyspace (cratestack#871)

> **Status: decided and implemented.** The maintainer's decision is recorded on
> cratestack#871 (2026-09-03): "store-side cardinality budget + opt-in
> verified-principal path" — option 2 with 3, and 1 as opt-in. This document
> records the mechanism, the defaults and their rationale, and — the part that
> matters most for a security control — what it does **not** bound.

## Problem

`RateLimitLayer` is a `tower::Layer` applied *outside* the generated router.
Authentication in this framework runs *inside* the generated handlers. So the
`Authorization` header that `default_key_fn` hashes into a bucket key has been
validated by nobody.

Measured in the cratestack#846 security review (quoted in that PR's §6): 20
requests with a rotating unvalidated bearer token → 20/20 allowed, **20 distinct
buckets**, each with a ≥60s TTL. Driving a real Redis to `maxmemory` that way
made every subsequent `HSET` fail. Under `main` at the time that was a DoS (500s
on every rate-limited route); after cratestack#846 it is a denied request — still
an attacker-chosen outage, and a log/metric amplifier.

The in-memory store had the matching defect from the other direction: a
`HashMap` that only ever grew, with no eviction at all.

cratestack#846 stopped the amplification from *disabling* the limiter.
cratestack#871 closes the primitive.

## Mechanism

Key derivation returns a key **plus** a `cratestack_core::BucketBudget`:

```rust
pub struct BucketBudget {
    pub scope_key: String,     // what is being counted, e.g. one peer
    pub fallback_key: String,  // what to charge once the scope is full
    pub max_distinct: u32,
    pub window: Duration,      // fixed, not sliding
}
```

The **store** applies it, in the same operation as the token consumption:

- **Redis** — one Lua script, three `KEYS` (requested bucket, scope set,
  fallback bucket). `SISMEMBER` hit → charge the requested bucket; else
  `SCARD < max_distinct` → `SADD` (plus a window `PEXPIRE` when the set is
  created) and charge it; else charge the fallback. Zero extra round-trips.
- **In-memory** — a `HashSet` per scope behind the same mutex as the buckets,
  plus an amortised sweep (at most once per 60s) that drops buckets idle for a
  full `cratestack_core::bucket_ttl_secs`, and a hard `max_buckets` cap that
  fails closed.

**Why the store and not the middleware.** Deciding in the layer needs a second
round-trip *and* races: N concurrent requests each read "under budget" and each
mint a bucket, which is the amplification reintroduced as a race. Atomicity is
the whole point, so the decision lives where atomicity is available.

Third-party stores keep compiling: `consume_bounded` has a default that
delegates to `consume` and reports `Charged::Unbounded`. The layer then emits a
throttled `WARN` naming the situation rather than letting a deployment believe
it is bounded.

## Scopes and defaults

| Request carries | Bucket key | Scope | Cap | Fallback |
|---|---|---|---|---|
| `VerifiedPrincipal` extension | `princ:<sha256>` | — | none | — |
| `Authorization` + `ConnectInfo` | `auth:<sha256>` | `peer:<ip>`, IPv6 → `/64` | **128 / 60s** | `ip:<ip>` |
| `Authorization`, no `ConnectInfo` | `auth:<sha256>` | `global` | **8192 / 60s** | `overflow` |
| `ConnectInfo` only | `ip:<ip>` | — | none | — |
| neither | refused, `412` (cratestack#416) | | | |

**128 per peer / 60s.** No realistic legitimate peer reaches it — 128
simultaneously-active distinct credentials from one NAT egress inside a minute
is already a deployment that should be configuring this rather than inheriting
it. An attacker needs one bucket per *request*, so the cap is orders of
magnitude below what the attack needs and above what real traffic uses.

**8192 globally / 60s.** Applies only when no verified peer address exists at
all (an unconfigured `into_make_service_with_connect_info`). Collateral there
falls on *unrelated* callers, so the cap is far looser: the global scope
degrades to one loud `overflow` bucket only when the deployment is both
misconfigured and under attack.

**Collapse, never refuse.** Refusing past the cap was considered and rejected:
it hands an attacker a deterministic, global outage of every rate-limited route
— the exact failure mode cratestack#846 was fought over. Collapsing onto the
caller's *own* `ip:` bucket throttles the attacker without giving them a lever
over anyone else's availability.

**cratestack#416 is preserved.** Under the cap, distinct callers never share.
An already-admitted member stays admitted for the rest of the window even while
its scope is saturated, so an attacker filling a peer's budget cannot displace
the legitimate callers that were in it first.

**IPv6 aggregated to /64, IPv4 not.** A /64 is the smallest block routinely
delegated to one subscriber, so without aggregation an attacker with one
ordinary residential prefix has 2^64 "peers" and the per-peer cap costs them
nothing. IPv4 gets no aggregation: a /24 under CGNAT is thousands of unrelated
subscribers, and IPv4 offers no comparable free-address supply. Note the
aggregation applies to the *scope* only — the fallback bucket is still the exact
address, because aggregating the throttling bucket itself would recreate
cratestack#416's collision across a whole prefix.

**Verified principals are opt-in, not the default.** `VerifiedPrincipal` is the
strongest option (nothing caller-mintable enters the key), but authentication
runs after this layer, so making it mandatory would collapse every existing
consumer's authenticated traffic onto `ip:` overnight.
`UnverifiedAuthPolicy::Ignore` is the middle setting: ignore the header, key on
the peer.

**In-memory `max_buckets` defaults to 100 000 and fails closed** with
`CratestackError::Internal` — a *logical* class, therefore refused under every
`StoreErrorPolicy`. Returning `Unavailable` there would hand an attacker "fill
the map, then walk through unthrottled", which is cratestack#846's bypass in a
different hat. The cap refuses only the marginal *new* identity; buckets that
already exist keep being served.

## What is NOT bounded

Stated plainly, because a security control that overstates its coverage is worse
than one that does not exist:

- **Distinct peers.** A botnet with N source addresses gets N per-peer budgets.
  The bound is O(peers × cap), not O(1). The in-memory `max_buckets` cap is the
  only backstop, and it is a fail-closed one.
- **Third-party stores that do not implement `consume_bounded`.** They keep
  working, unbounded, with a throttled `WARN` per hour. The trait cannot force
  the atomicity the bound needs.
- **`with_key_fn` overrides.** The layer has no basis to invent a scope or a
  fallback for a derivation it cannot see. A consumer whose key function reads
  caller-supplied material owns bounding it, exactly as it already owns the
  fail-closed decision.
- **Redis Cluster.** Three un-hash-tagged keys in one script means `CROSSSLOT`,
  refused loudly as a logical-class error. Forcing them into one slot would
  concentrate an attacker's traffic on a single node. Cluster is not a supported
  deployment for this store today.
- **The fixed window's boundary.** Up to `2 × max_distinct` buckets can be alive
  across a window rollover. That is a constant factor on a bound whose purpose
  is to replace "unbounded" with "constant"; a sliding window would cost a
  sorted set and a range trim per request.
- **Scope-set memory itself.** Each admitted member is one entry in a set that
  expires with its window. It is proportional to the buckets it indexes, not
  independent of them, but it is real added memory.

## References

- cratestack#871 (ticket and the maintainer decision comment), cratestack#846
  (PR §6, finding 1 — the measured probe), cratestack#416 (why key derivation
  is fail-closed).
- `crates/cratestack-core/src/store/ratelimit/budget.rs`,
  `crates/cratestack-axum/src/ratelimit/{scope,budget,key_fn,consume}.rs` and
  `.../store/{buckets,scopes}.rs`,
  `crates/cratestack-redis/src/ratelimit/scripts.rs`.
- `docs/design/trusted-proxy-client-ip.md` — why forwarded headers are never a
  substitute for `ConnectInfo` here.
