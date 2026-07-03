#[cfg(test)]
mod tests {
    use voltgate::retry::{should_retry, backoff_delay, with_retry, RetryDecision, MAX_ATTEMPTS};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn overloaded_529_is_retried() { assert_eq!(should_retry(529), RetryDecision::Retry); }
    #[test]
    fn internal_error_500_is_retried() { assert_eq!(should_retry(500), RetryDecision::Retry); }
    #[test]
    fn bad_gateway_502_is_retried() { assert_eq!(should_retry(502), RetryDecision::Retry); }
    #[test]
    fn service_unavailable_503_is_retried() { assert_eq!(should_retry(503), RetryDecision::Retry); }
    #[test]
    fn gateway_timeout_504_is_retried() { assert_eq!(should_retry(504), RetryDecision::Retry); }
    #[test]
    fn rate_limited_429_is_retried() { assert_eq!(should_retry(429), RetryDecision::Retry); }
    #[test]
    fn bad_request_400_gives_up() { assert_eq!(should_retry(400), RetryDecision::GiveUp); }
    #[test]
    fn unauthorized_401_gives_up() { assert_eq!(should_retry(401), RetryDecision::GiveUp); }
    #[test]
    fn forbidden_403_gives_up() { assert_eq!(should_retry(403), RetryDecision::GiveUp); }
    #[test]
    fn not_found_404_gives_up() { assert_eq!(should_retry(404), RetryDecision::GiveUp); }

    #[test]
    fn backoff_delay_increases_with_attempt() {
        let d0 = backoff_delay(0);
        let d1 = backoff_delay(1);
        let d2 = backoff_delay(2);
        // Even with jitter, later attempts should generally have a higher base
        assert!(d1.as_millis() >= 350, "attempt 1 should be roughly 2x base: {d1:?}");
        assert!(d2.as_millis() >= 700, "attempt 2 should be roughly 4x base: {d2:?}");
        let _ = d0;
    }

    #[test]
    fn backoff_delay_has_jitter_variance() {
        // Run multiple times, expect not all identical (jitter is random)
        let delays: Vec<u128> = (0..10).map(|_| backoff_delay(1).as_millis()).collect();
        let all_same = delays.windows(2).all(|w| w[0] == w[1]);
        assert!(!all_same, "jitter should produce varying delays");
    }

    #[tokio::test]
    async fn succeeds_immediately_no_retry_needed() {
        let outcome = with_retry(|_attempt| async { Ok::<_, (u16, String)>(42) }).await;
        assert_eq!(outcome.result.unwrap(), 42);
        assert_eq!(outcome.attempts, 1);
        assert!(!outcome.retried);
    }

    #[tokio::test]
    async fn retries_on_529_then_succeeds() {
        let call_count = Arc::new(AtomicU32::new(0));
        let cc = call_count.clone();
        let outcome = with_retry(|_attempt| {
            let cc = cc.clone();
            async move {
                let n = cc.fetch_add(1, Ordering::SeqCst);
                if n < 2 { Err((529u16, "overloaded".to_string())) } else { Ok::<_, (u16, String)>("success") }
            }
        }).await;
        assert_eq!(outcome.result.unwrap(), "success");
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
        assert!(outcome.retried);
    }

    #[tokio::test]
    async fn gives_up_immediately_on_400() {
        let call_count = Arc::new(AtomicU32::new(0));
        let cc = call_count.clone();
        let outcome = with_retry(|_attempt| {
            let cc = cc.clone();
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>((400u16, "bad request".to_string()))
            }
        }).await;
        assert!(outcome.result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1, "should not retry a 400");
    }

    #[tokio::test]
    async fn exhausts_max_attempts_on_persistent_529() {
        let call_count = Arc::new(AtomicU32::new(0));
        let cc = call_count.clone();
        let outcome = with_retry(|_attempt| {
            let cc = cc.clone();
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>((529u16, "still overloaded".to_string()))
            }
        }).await;
        assert!(outcome.result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), MAX_ATTEMPTS);
        assert_eq!(outcome.attempts, MAX_ATTEMPTS);
    }

    #[tokio::test]
    async fn network_error_status_zero_is_retried() {
        let call_count = Arc::new(AtomicU32::new(0));
        let cc = call_count.clone();
        let outcome = with_retry(|_attempt| {
            let cc = cc.clone();
            async move {
                let n = cc.fetch_add(1, Ordering::SeqCst);
                if n < 1 { Err((0u16, "connection reset".to_string())) } else { Ok::<_, (u16, String)>("ok") }
            }
        }).await;
        assert!(outcome.result.is_ok());
        assert!(call_count.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn error_message_preserved_on_final_failure() {
        let outcome = with_retry(|_attempt| async {
            Err::<i32, _>((400u16, "specific validation error".to_string()))
        }).await;
        assert!(outcome.result.unwrap_err().contains("specific validation error"));
    }
}
