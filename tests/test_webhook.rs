#[cfg(test)]
mod tests {
    use voltgate::webhook::{check_threshold_crossed, ALERT_THRESHOLDS};

    #[test]
    fn thresholds_are_80_and_100_percent() {
        assert_eq!(ALERT_THRESHOLDS, [0.80, 1.00]);
    }

    #[test]
    fn no_alert_below_threshold() {
        // Use a unique caller id per test to avoid cross-test interference
        // (the alerted-set is a shared static).
        let result = check_threshold_crossed("webhook-test-below", 5.0, 100.0);
        assert!(result.is_none(), "5% spend should not trigger any alert");
    }

    #[test]
    fn alert_fires_at_80_percent() {
        let result = check_threshold_crossed("webhook-test-80pct", 8.0, 10.0);
        assert_eq!(result, Some(0.80));
    }

    #[test]
    fn alert_fires_at_100_percent() {
        // First crossing 80%, then crossing 100% should each fire once.
        let _ = check_threshold_crossed("webhook-test-100pct", 8.0, 10.0);
        let result = check_threshold_crossed("webhook-test-100pct", 10.0, 10.0);
        assert_eq!(result, Some(1.00));
    }

    #[test]
    fn duplicate_alert_not_fired_twice_in_same_hour() {
        let caller = "webhook-test-dedup";
        let first  = check_threshold_crossed(caller, 8.5, 10.0);
        let second = check_threshold_crossed(caller, 9.0, 10.0); // still only past 80%, not 100%
        assert_eq!(first, Some(0.80));
        assert_eq!(second, None, "should not re-fire the 80% alert");
    }

    #[test]
    fn zero_limit_never_alerts() {
        let result = check_threshold_crossed("webhook-test-zero-limit", 5.0, 0.0);
        assert!(result.is_none());
    }

    #[test]
    fn negative_limit_never_alerts() {
        let result = check_threshold_crossed("webhook-test-neg-limit", 5.0, -10.0);
        assert!(result.is_none());
    }

    #[test]
    fn different_callers_alert_independently() {
        let a = check_threshold_crossed("webhook-test-caller-a", 9.0, 10.0);
        let b = check_threshold_crossed("webhook-test-caller-b", 9.0, 10.0);
        // Both should independently cross 80% since they're different callers
        assert_eq!(a, Some(0.80));
        assert_eq!(b, Some(0.80));
    }

    #[test]
    fn spend_over_100_percent_still_reports_100_not_higher() {
        // Even at 150% of budget, the highest defined threshold (100%) fires,
        // not some undefined higher value.
        let result = check_threshold_crossed("webhook-test-over-limit", 15.0, 10.0);
        assert_eq!(result, Some(1.00));
    }

    #[test]
    fn exactly_at_threshold_boundary_fires() {
        // Exactly 80.0% of limit should trigger, not require >80%.
        let result = check_threshold_crossed("webhook-test-exact-80", 80.0, 100.0);
        assert_eq!(result, Some(0.80));
    }
}
