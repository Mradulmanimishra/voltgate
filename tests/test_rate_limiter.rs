#[cfg(test)]
mod tests {
    use voltgate::rate_limiter::RateLimiter;

    #[tokio::test]
    async fn first_request_always_passes() {
        assert!(RateLimiter::new().check_request("caller-a", 10).await.is_ok());
    }
    #[tokio::test]
    async fn requests_under_limit_all_pass() {
        let rl = RateLimiter::new();
        for _ in 0..5 { assert!(rl.check_request("caller-b", 5).await.is_ok()); }
    }
    #[tokio::test]
    async fn request_over_limit_is_rejected() {
        let rl = RateLimiter::new();
        for _ in 0..3 { let _ = rl.check_request("caller-c", 3).await; }
        assert!(rl.check_request("caller-c", 3).await.is_err());
    }
    #[tokio::test]
    async fn different_callers_have_independent_limits() {
        let rl = RateLimiter::new();
        for _ in 0..2 { let _ = rl.check_request("caller-d", 2).await; }
        assert!(rl.check_request("caller-d", 2).await.is_err());
        assert!(rl.check_request("caller-e", 2).await.is_ok());
    }
    #[tokio::test]
    async fn zero_max_rpm_blocks_everything() {
        assert!(RateLimiter::new().check_request("caller-f", 0).await.is_err());
    }
    #[tokio::test]
    async fn rate_limit_error_message_contains_limit() {
        let rl = RateLimiter::new();
        let _ = rl.check_request("caller-g", 1).await;
        let err = rl.check_request("caller-g", 1).await.unwrap_err();
        assert!(err.to_string().contains('1') || err.to_string().contains("limit"));
    }
    #[tokio::test]
    async fn first_spend_passes_if_under_limit() {
        assert!(RateLimiter::new().record_spend("spender-a", 0.50, 10.0).await.is_ok());
    }
    #[tokio::test]
    async fn spend_exactly_at_limit_passes() {
        let rl = RateLimiter::new();
        let _ = rl.record_spend("spender-b", 5.0, 10.0).await;
        assert!(rl.record_spend("spender-b", 5.0, 10.0).await.is_ok());
    }
    #[tokio::test]
    async fn spend_over_limit_is_rejected() {
        let rl = RateLimiter::new();
        let _ = rl.record_spend("spender-c", 8.0, 10.0).await;
        assert!(rl.record_spend("spender-c", 5.0, 10.0).await.is_err());
    }
    #[tokio::test]
    async fn zero_spend_always_passes() {
        let rl = RateLimiter::new();
        for _ in 0..100 { assert!(rl.record_spend("spender-d", 0.0, 1.0).await.is_ok()); }
    }
    #[tokio::test]
    async fn different_callers_spend_independently() {
        let rl = RateLimiter::new();
        let _ = rl.record_spend("spender-e", 9.0, 10.0).await;
        assert!(rl.record_spend("spender-f", 9.0, 10.0).await.is_ok());
    }
    #[tokio::test]
    async fn cumulative_spend_is_tracked_correctly() {
        let rl = RateLimiter::new();
        let r1 = rl.record_spend("spender-g", 1.0, 100.0).await.unwrap();
        let r2 = rl.record_spend("spender-g", 2.0, 100.0).await.unwrap();
        let r3 = rl.record_spend("spender-g", 3.0, 100.0).await.unwrap();
        assert!((r1 - 1.0).abs() < 1e-6);
        assert!((r2 - 3.0).abs() < 1e-6);
        assert!((r3 - 6.0).abs() < 1e-6);
    }
    #[tokio::test]
    async fn spend_error_mentions_amount() {
        let rl = RateLimiter::new();
        let _ = rl.record_spend("spender-h", 9.0, 10.0).await;
        let err = rl.record_spend("spender-h", 5.0, 10.0).await.unwrap_err();
        assert!(err.to_string().contains("10") || err.to_string().contains("spend"));
    }
    #[tokio::test]
    async fn snapshot_includes_known_caller() {
        let rl = RateLimiter::new();
        let _ = rl.check_request("snapshot-caller", 100).await;
        let _ = rl.record_spend("snapshot-caller", 2.50, 100.0).await;
        assert!(rl.snapshot().await.iter().any(|(id, _, _)| id == "snapshot-caller"));
    }
    #[tokio::test]
    async fn snapshot_spend_matches_recorded() {
        let rl = RateLimiter::new();
        let _ = rl.check_request("spend-snap", 100).await;
        let _ = rl.record_spend("spend-snap", 3.75, 100.0).await;
        if let Some((_, _, spend)) = rl.snapshot().await.iter().find(|(id, _, _)| id == "spend-snap") {
            assert!((*spend - 3.75).abs() < 1e-6);
        }
    }
}
