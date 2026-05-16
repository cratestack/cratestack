use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cratestack_core::CoolError;

pub(super) fn system_time_to_ms(time: SystemTime) -> Result<i64, CoolError> {
    let dur = time.duration_since(UNIX_EPOCH).map_err(|err| {
        CoolError::Internal(format!(
            "redis idempotency: timestamp before unix epoch: {err}"
        ))
    })?;
    i64::try_from(dur.as_millis()).map_err(|_| {
        CoolError::Internal("redis idempotency: timestamp out of i64 ms range".to_owned())
    })
}

pub(super) fn system_time_from_ms(ms: i64) -> SystemTime {
    if ms >= 0 {
        UNIX_EPOCH + Duration::from_millis(ms as u64)
    } else {
        UNIX_EPOCH - Duration::from_millis(ms.unsigned_abs())
    }
}
