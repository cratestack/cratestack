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
    pub window: Duration,      // FLOOR on the record's lifetime
}
```

The **store** applies it, in the same operation as the token consumption:

- **Redis** — one Lua script, three `KEYS` (requested bucket, scope ZSET,
  fallback bucket). `ZREMRANGEBYSCORE` trims aged-out slots; `ZSCORE` hit →
  refresh the score and charge the requested bucket; else `ZCARD <
  max_distinct` → `ZADD` and charge it; else charge the fallback. The scope key
  is `PEXPIRE`d on every hit. Zero extra round-trips.
- **In-memory** — a `HashSet` per scope behind the same mutex as the buckets,
  plus an amortised sweep (at most once per 60s) that drops buckets idle for a
  full `cratestack_core::bucket_ttl_secs`, and a hard `max_buckets` cap that
  fails closed.

**The scope record must outlive the buckets it admitted.** This is not a detail;
getting it wrong voids the whole bound. The first implementation gave the scope
a fixed `window` lifetime (Redis: a `<epoch>`-suffixed key with
`PEXPIRE window_ms`) while buckets got `EXPIRE bucket_ttl`. With
`window < bucket_ttl`, every rollover minted a fresh record that re-admitted
`max_distinct` more buckets on top of a generation that was still alive — a real
steady state of `max_distinct × ceil(bucket_ttl / window)`. Measured: **21
buckets for a cap of 4 over five 1s windows** (81 over twenty), and ~184 320 per
peer for a non-refilling bucket under the defaults, which overruns the in-memory
cap on its own. The lifetime is now `cratestack_core::scope_ttl_secs` —
`max(window, bucket_ttl_secs(config))`, clamped to a year — refreshed on every
admission, and the Redis key carries no epoch. Dropping the epoch also removes a
clock-skew multiplier: replicas disagreeing on `now_ms` used to land on
different epoch keys and each mint their own generation.

### The window slides, per member

One shared deadline for the whole scope forced a choice between two defects.
Refreshing it on every *hit* pinned a saturated scope open forever, capping a
token-rotating deployment at its first `max_distinct` credentials. Refreshing
only on *admission* let the record expire while a bucket it admitted was still
alive (kept warm by traffic), freeing a slot for another alongside it — a
transient `2 × max_distinct`.

Per-member expiry removes the choice, so the "sliding window" this document
previously ruled out is now what ships. The scope is a sorted set scored by
**last use**; each slot expires `scope_ttl` after its credential was last seen.
An actively-used member's score is refreshed, so its slot is never freed while
its bucket is alive; one that goes quiet is trimmed, so rotation reclaims it.
The bound is `max_distinct` live buckets per scope at *every* instant, not just
in steady state. Cost is one extra O(log N) command at N ≤ 128, in the same
round-trip.

Scoring by first admission rather than last use would reintroduce the `2N`
exactly, which is why the score is written on every hit.

Redis Lua is atomic but **not transactional**: a script that aborts partway (a
mid-script `OOM`) can leave a set that was `SADD`ed but never `PEXPIRE`d, and
therefore never expires. Re-`PEXPIRE`ing on every admission repairs exactly that
on the next admission, which is the second reason the refresh is unconditional.

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
| `Authorization` + `ConnectInfo` | `auth:<sha256>` | `peer:<addr>` | **128** | `ip:<addr>` |
| `Authorization`, no `ConnectInfo` | `auth:<sha256>` | `global` | **8192** | `overflow` |
| `ConnectInfo` only | `ip:<addr>` | — | none | — |
| neither | *refused, `412`* (cratestack#416) | — | — | — |

`<addr>` is the peer address for IPv4 and its **/64 prefix** for routable IPv6.
IPv4-mapped addresses (`::ffff:a.b.c.d`) are unwrapped to their IPv4 form
first — see the IPv6 section below.

**128 per peer.** No realistic legitimate peer reaches it — 128
simultaneously-active distinct credentials from one NAT egress (or one IPv6
/64) within a slot lifetime is already a deployment that should be configuring
this rather than inheriting it. An attacker needs one bucket per *request*, so the cap is orders of
magnitude below what the attack needs and above what real traffic uses.

**8192 globally.** Applies only when no verified peer address exists at
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
An already-admitted member keeps its slot for as long as it keeps using it,
even while its scope is saturated, so an attacker filling a peer's budget cannot displace
the legitimate callers that were in it first.

**IPv6 aggregated to /64 EVERYWHERE the address becomes a key, IPv4 not.** A
/64 is the smallest block routinely delegated to one subscriber, so without
aggregation an attacker with one ordinary residential prefix has 2^64 "peers"
and the per-peer cap costs them nothing. IPv4 gets no aggregation: a /24 under
CGNAT is thousands of unrelated subscribers, and IPv4 offers no comparable
free-address supply.

The first implementation aggregated only the *scope* and left the `ip:` fallback
and the no-`Authorization` bucket on the full address. That bounded nothing, and
it was measured: rotating the source address inside a single /64 produced **200
buckets** with a token at cap 8, and **200 buckets, 200/200 allowed**, with no
`Authorization` header at all — the cratestack#846 signature with the address as
the rotating variable. Aggregation now applies to the scope key, the fallback
bucket key, and the unauthenticated bucket key alike.

**IPv4-mapped addresses are unwrapped first.** A dual-stack listener —
`TcpListener::bind("[::]:0")`, the ordinary Linux bind — delivers every IPv4
client as `::ffff:a.b.c.d`, whose top four groups are zero. Taking the /64
blindly therefore mapped **every IPv4 client onto `ip:::/64`**: measured, 200
distinct IPv4 clients collapsed into 1 bucket with 5 allowed — a collision of
unlimited width and a one-client denial of service against all IPv4 traffic,
strictly worse than the evasion the aggregation closed. `to_ipv4_mapped()` is
applied before aggregating, and `to_ipv4()` is deliberately NOT used: it also
accepts the deprecated IPv4-compatible form, mapping `::1` to `0.0.0.1` and
`::` to `0.0.0.0` — trading one collision for another. Instead the whole
all-zero `::/64` region is exempted from aggregation and keyed per-address;
nothing in it is globally routable, so it offers no address supply to rotate
through.

**The accepted trade-off, stated rather than hidden:** two distinct hosts inside
one *routable* IPv6 /64 share a throttling bucket. That is a genuine cratestack#416
collision, taken deliberately because a /64 is one subscriber and an attacker's
2^64-address supply is not hypothetical.

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
  The bound is O(peers × cap), not O(1). In the in-memory store the
  `max_buckets` cap backstops both the bucket and the scope map, fail-closed;
  Redis has no such cap and relies on the per-key TTLs.
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
- **Distinct hosts inside one routable IPv6 /64.** They share a bucket, by
  design (see above). This is a cratestack#416 collision accepted in exchange
  for closing the address-rotation evasion. It applies to *routable* IPv6 only:
  IPv4, IPv4-mapped addresses and the whole `::/64` special region are keyed
  per-address.
- ~~A transient overlap of up to `2 × max_distinct`.~~ **Closed** by the
  sliding window (see "The window slides, per member"): a member's slot is
  refreshed on use, so it can never expire while its bucket is alive, and the
  bound `max_distinct + 2` now holds at every instant rather than only in steady
  state.
- **Scope memory is bounded but not free.** Each admitted member is one entry
  in its scope's sorted set, expiring individually. It is NOT merely
  "proportional to the buckets it indexes" — an earlier revision claimed that
  and it was false: scope entries outlive their buckets by design (`scope_ttl`
  ≥ bucket TTL), and admission used to happen before the bucket cap could
  refuse, so a capped in-memory store measured 10 buckets against 5000 scopes.
  Admission is now gated on the bucket being creatable, and the scope map has
  its own `max_buckets`-derived cap that fails closed — but the two counts are
  separate quantities and are asserted separately (`_bucket_count`,
  `_scope_count`).
- **A scope set that lost its TTL to an aborted script.** Redis Lua is atomic
  but not transactional, so a mid-script failure between `SADD` and `PEXPIRE`
  leaves a set with no expiry. The next admission on that scope re-`PEXPIRE`s
  it, so it is self-repairing — but a scope that is never admitted to again
  keeps one set until the instance is flushed.

## References

- cratestack#871 (ticket, the maintainer decision comment, and the adversarial
  review of PR #880 that produced the two blockers recorded above),
  cratestack#846 (PR §6, finding 1 — the measured probe), cratestack#416 (why
  key derivation is fail-closed).
- `crates/cratestack-core/src/store/ratelimit/budget.rs`,
  `crates/cratestack-axum/src/ratelimit/{scope,budget,key_fn,consume}.rs` and
  `.../store/{buckets,scopes}.rs`,
  `crates/cratestack-redis/src/ratelimit/scripts.rs`.
- `docs/design/trusted-proxy-client-ip.md` — why forwarded headers are never a
  substitute for `ConnectInfo` here.
