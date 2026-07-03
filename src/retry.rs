/// retry.rs — exponential backoff retry wrapper for Anthropic API calls.
/// Retries 529 (overloaded), 500/502/503/504, and 429 (rate limited).
/// Does NOT retry 4xx client errors (400/401/403/404) — those won't
/// succeed on retry and would just waste time and money.
/// Backoff: 200ms → 400ms → 800ms → 1600ms with ±20% jitter.

use std::time::Duration;
use rand::Rng;

pub const MAX_ATTEMPTS: u32 = 4;
const BASE_DELAY_MS: u64 = 200;
const JITTER_PCT: f64 = 0.20;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RetryDecision { Retry, GiveUp }

pub fn should_retry(status: u16) -> RetryDecision {
    match status {
        529 | 500 | 502 | 503 | 504 | 429 => RetryDecision::Retry,
        _ => RetryDecision::GiveUp,
    }
}

pub fn backoff_delay(attempt: u32) -> Duration {
    let base = BASE_DELAY_MS * 2_u64.pow(attempt);
    let jitter_range = (base as f64 * JITTER_PCT) as u64;
    let jitter = if jitter_range > 0 { rand::thread_rng().gen_range(0..=jitter_range) } else { 0 };
    Duration::from_millis(base + jitter)
}

pub struct RetryOutcome<T> {
    pub result:   Result<T, String>,
    pub attempts: u32,
    pub retried:  bool,
}

pub async fn with_retry<T, F, Fut>(mut f: F) -> RetryOutcome<T>
where
    F:   FnMut(u32) -> Fut,
    Fut: std::future::Future<Output = Result<T, (u16, String)>>,
{
    let mut last_err = String::new();
    let mut attempts  = 0;

    for attempt in 0..MAX_ATTEMPTS {
        attempts += 1;
        match f(attempt).await {
            Ok(value) => return RetryOutcome { result: Ok(value), attempts, retried: attempt > 0 },
            Err((status, msg)) => {
                last_err = msg;
                let decision = if status == 0 { RetryDecision::Retry } else { should_retry(status) };
                if decision == RetryDecision::GiveUp || attempt == MAX_ATTEMPTS - 1 { break; }
                let delay = backoff_delay(attempt);
                tracing::warn!(attempt = attempt + 1, max = MAX_ATTEMPTS, status, delay_ms = delay.as_millis(), "Retrying after transient error");
                tokio::time::sleep(delay).await;
            }
        }
    }
    RetryOutcome { result: Err(last_err), attempts, retried: attempts > 1 }
}
