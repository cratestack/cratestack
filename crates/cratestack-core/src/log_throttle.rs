//! A "log at most once per interval, and say how many you swallowed"
//! counter (cratestack#846).
//!
//! Exists because the two failure paths that most need a log line are
//! also the two an attacker can drive: a rate-limit store outage emits
//! one `WARN` per request, and the request rate during an outage is
//! whatever the caller chooses. An unthrottled per-request `WARN` turns
//! a store outage into a log-volume amplifier on top of everything else
//! — the incident that produced this crate's fail-open policy would have
//! written one line per request for as long as Redis was down.
//!
//! Deliberately not a general-purpose rate limiter: no token bucket, no
//! configuration, no allocation. The suppressed count is what makes the
//! throttle honest — an operator reading the log sees "…and 4,812
//! more", not a single line that understates the blast radius.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Decides whether the caller should emit its log line now, and how many
/// it has swallowed since the last one it allowed.
#[derive(Debug)]
pub struct LogThrottle {
    interval: Duration,
    state: Mutex<ThrottleState>,
}

#[derive(Debug)]
struct ThrottleState {
    last_emitted: Option<Instant>,
    suppressed: u64,
}

/// Returned by [`LogThrottle::check`]: either "emit, and mention this
/// many suppressed since last time" or "stay quiet".
#[derive(Debug, PartialEq, Eq)]
pub enum ThrottleDecision {
    Emit { suppressed_since_last: u64 },
    Suppress,
}

impl LogThrottle {
    pub const fn new(interval: Duration) -> Self {
        Self {
            interval,
            state: Mutex::new(ThrottleState {
                last_emitted: None,
                suppressed: 0,
            }),
        }
    }

    /// The first call always emits — an operator must never have to wait
    /// out an interval to learn that something started failing.
    pub fn check(&self) -> ThrottleDecision {
        self.check_at(Instant::now())
    }

    /// Injection point for tests: the whole type is a clock decision, and
    /// sleeping through real intervals in a test suite is how flakes are
    /// born.
    pub fn check_at(&self, now: Instant) -> ThrottleDecision {
        // A poisoned mutex here must not take down the caller's request
        // path — this is a logging aid, not a correctness primitive — so
        // recover the guard rather than propagating the panic.
        let mut state = self.state.unwrap_or_recover();
        let due = match state.last_emitted {
            None => true,
            Some(last) => now.saturating_duration_since(last) >= self.interval,
        };
        if due {
            let suppressed_since_last = std::mem::take(&mut state.suppressed);
            state.last_emitted = Some(now);
            ThrottleDecision::Emit {
                suppressed_since_last,
            }
        } else {
            state.suppressed = state.suppressed.saturating_add(1);
            ThrottleDecision::Suppress
        }
    }
}

/// Tiny extension so the poisoning recovery above reads as one call
/// rather than three lines of `match` at the only site that needs it.
trait UnwrapOrRecover<'a, T> {
    fn unwrap_or_recover(&'a self) -> std::sync::MutexGuard<'a, T>;
}

impl<'a, T> UnwrapOrRecover<'a, T> for Mutex<T> {
    fn unwrap_or_recover(&'a self) -> std::sync::MutexGuard<'a, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests;
