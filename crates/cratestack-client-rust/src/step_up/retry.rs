//! Retry a call once after a re-proof (cratestack#1244).

use std::future::{Future, IntoFuture};

use super::{StepUpError, StepUpRequired};

/// Run `call` (a future, or anything `.await` takes, such as a generated
/// RPC method's call);  when it fails with a step-up refusal, hand the refusal to
/// `reprove` and, when that answers `true` (the caller authenticated
/// again), run `call` **once** more and return what it gives, even a
/// second step-up refusal. Never loops.
///
/// `reprove` answers `false` when the user declined or the re-proof
/// failed; the first refusal is then returned unchanged. Any other error,
/// and a success, is returned from the first call as it is.
///
/// ```ignore
/// let order = retry_after_step_up(
///     || client.procedures().transfer(&args),
///     |step_up| async move { session.reauthenticate(step_up.max_age).await.is_ok() },
/// )
/// .await?;
/// ```
pub async fn retry_after_step_up<T, E, F, Fut, R, RFut>(mut call: F, reprove: R) -> Result<T, E>
where
    E: StepUpError,
    F: FnMut() -> Fut,
    Fut: IntoFuture<Output = Result<T, E>>,
    R: FnOnce(StepUpRequired) -> RFut,
    RFut: Future<Output = bool>,
{
    let error = match call().await {
        Ok(value) => return Ok(value),
        Err(error) => error,
    };
    let Some(step_up) = error.step_up() else {
        return Err(error);
    };
    if !reprove(step_up).await {
        return Err(error);
    }
    call().await
}
