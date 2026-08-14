use std::time::{SystemTime, UNIX_EPOCH};

use cratestack_core::CratestackError;

pub(super) fn system_time_to_ms(time: SystemTime) -> Result<i64, CratestackError> {
    let dur = time.duration_since(UNIX_EPOCH).map_err(|err| {
        CratestackError::Internal(format!(
            "redis rate limit: timestamp before unix epoch: {err}"
        ))
    })?;
    i64::try_from(dur.as_millis()).map_err(|_| {
        CratestackError::Internal("redis rate limit: timestamp out of i64 ms range".to_owned())
    })
}
