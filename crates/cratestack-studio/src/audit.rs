//! In-memory audit ring buffer for Studio writes.
//!
//! Phase 4 adds an opt-in (always-on, but bounded) record of every
//! successful CREATE / UPDATE / DELETE the studio performs. Entries
//! live in process memory only — there's no on-disk persistence by
//! design. The buffer caps at [`AuditLog::CAPACITY`]; older entries
//! are dropped FIFO once the cap is reached.
//!
//! Studio is a local admin tool, so a small bounded buffer is fine:
//! the operator can see what they (or a teammate connected to the
//! same target) just did without us inventing a logging pipeline.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// One captured write. `at` is an RFC-3339 timestamp; `pk` is the
/// row's primary-key value after the write (so for CREATE we capture
/// the generated value if the DB filled one in).
#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub id: u64,
    pub at: String,
    pub target: String,
    pub model: String,
    pub op: AuditOp,
    pub pk: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AuditOp {
    Create,
    Update,
    Delete,
}

#[derive(Debug)]
pub struct AuditLog {
    entries: Mutex<VecDeque<AuditEntry>>,
    next_id: Mutex<u64>,
}

impl AuditLog {
    pub const CAPACITY: usize = 500;

    pub fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(Self::CAPACITY)),
            next_id: Mutex::new(1),
        }
    }

    pub fn push(&self, target: &str, model: &str, op: AuditOp, pk: Option<String>) {
        let id = {
            let mut next = self.next_id.lock().expect("audit id mutex poisoned");
            let id = *next;
            *next += 1;
            id
        };
        let entry = AuditEntry {
            id,
            at: now_rfc3339(),
            target: target.to_owned(),
            model: model.to_owned(),
            op,
            pk,
        };
        let mut entries = self.entries.lock().expect("audit mutex poisoned");
        if entries.len() == Self::CAPACITY {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// Snapshot the most recent `limit` entries in reverse-chronological
    /// order (newest first). `limit` is clamped to the buffer capacity.
    pub fn snapshot(&self, limit: usize) -> Vec<AuditEntry> {
        let entries = self.entries.lock().expect("audit mutex poisoned");
        let limit = limit.min(entries.len());
        entries.iter().rev().take(limit).cloned().collect()
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    rfc3339_from_unix(secs)
}

/// Minimal UTC RFC-3339 encoder so we don't need a chrono dep for one
/// timestamp. Handles dates from 1970 onward, which covers anything
/// `SystemTime::now()` produces on a running machine.
fn rfc3339_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let seconds_of_day = secs.rem_euclid(86_400) as u32;
    let (hour, rest) = (seconds_of_day / 3600, seconds_of_day % 3600);
    let (minute, second) = (rest / 60, rest % 60);

    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days` algorithm. Converts days from
/// 1970-01-01 to a (year, month, day) gregorian tuple.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_snapshot_return_newest_first() {
        let log = AuditLog::new();
        log.push("t", "Post", AuditOp::Create, Some("p1".to_owned()));
        log.push("t", "Post", AuditOp::Update, Some("p1".to_owned()));
        let snap = log.snapshot(10);
        assert_eq!(snap.len(), 2);
        assert!(matches!(snap[0].op, AuditOp::Update));
        assert!(matches!(snap[1].op, AuditOp::Create));
    }

    #[test]
    fn buffer_drops_oldest_past_capacity() {
        let log = AuditLog::new();
        for i in 0..AuditLog::CAPACITY + 5 {
            log.push("t", "Post", AuditOp::Create, Some(format!("p{i}")));
        }
        let snap = log.snapshot(AuditLog::CAPACITY * 2);
        assert_eq!(snap.len(), AuditLog::CAPACITY);
        // The first 5 entries should be gone (oldest dropped).
        assert!(snap.iter().all(|e| e.pk.as_deref() != Some("p0")));
        assert!(snap.iter().all(|e| e.pk.as_deref() != Some("p4")));
        assert!(snap.iter().any(|e| e.pk.as_deref() == Some("p5")));
    }

    #[test]
    fn rfc3339_encodes_epoch() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn rfc3339_encodes_known_date() {
        // 2024-01-15T12:34:56Z = 1705322096
        assert_eq!(rfc3339_from_unix(1_705_322_096), "2024-01-15T12:34:56Z");
    }
}
