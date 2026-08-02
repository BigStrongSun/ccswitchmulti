use std::future::Future;
use std::time::Duration;

use super::error::{ProxyError, RESPONSE_PENDING_GRACE_SECS};

/// 在返回 429 前继续等待上游迟到结果的宽限期。
pub(crate) const RESPONSE_PENDING_GRACE: Duration =
    Duration::from_secs(RESPONSE_PENDING_GRACE_SECS);

/// 常规超时后保留上游 future，继续等待宽限期。
///
/// 只有仍然持有原请求 future 时才能做到“上游晚到但结果仍可交付”。如果 timeout
/// 到期就返回并丢弃 future，上游后续的成功响应只能被取消/忽略。
pub(crate) async fn await_with_response_grace<F, T>(
    future: F,
    timeout: Duration,
    grace: Duration,
    error: impl FnOnce() -> ProxyError,
) -> Result<T, ProxyError>
where
    F: Future<Output = Result<T, ProxyError>>,
{
    if timeout.is_zero() {
        return future.await;
    }

    let mut future = std::pin::pin!(future);
    tokio::select! {
        result = future.as_mut() => result,
        _ = tokio::time::sleep(timeout) => {
            log::warn!(
                "[ResponseGrace] upstream did not finish within {}s; waiting up to {}s for a late result",
                timeout.as_secs(),
                grace.as_secs()
            );
            tokio::select! {
                result = future.as_mut() => {
                    log::info!("[ResponseGrace] recovered late upstream result");
                    result
                }
                _ = tokio::time::sleep(grace) => Err(error()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recovers_late_result_within_grace() {
        let result = await_with_response_grace(
            async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                Ok::<_, ProxyError>("late-ok")
            },
            Duration::from_millis(1),
            Duration::from_millis(50),
            || ProxyError::ResponsePending("should not happen".to_string()),
        )
        .await;

        assert_eq!(result.unwrap(), "late-ok");
    }

    #[tokio::test]
    async fn returns_pending_after_grace() {
        let result = await_with_response_grace(
            async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok::<_, ProxyError>("too-late")
            },
            Duration::from_millis(1),
            Duration::from_millis(5),
            || ProxyError::ResponsePending("grace expired".to_string()),
        )
        .await;

        assert!(matches!(
            result,
            Err(ProxyError::ResponsePending(msg)) if msg == "grace expired"
        ));
    }
}
