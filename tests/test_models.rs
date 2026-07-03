#[cfg(test)]
mod tests {
    use voltgate::models::*;

    #[test]
    fn haiku_pricing_is_correct() {
        let p = ModelPricing::for_model("claude-haiku-4-5");
        assert!((p.cost(1_000_000, 0) - 0.25).abs() < 1e-6);
    }
    #[test]
    fn sonnet_pricing_is_correct() {
        let p = ModelPricing::for_model("claude-sonnet-4-6");
        assert!((p.cost(1_000_000, 0) - 3.00).abs() < 1e-6);
    }
    #[test]
    fn fable_pricing_is_correct() {
        let p = ModelPricing::for_model("claude-fable-5");
        assert!((p.cost(1_000_000, 0) - 10.00).abs() < 1e-6);
    }
    #[test]
    fn opus_pricing_is_correct() {
        let p = ModelPricing::for_model("claude-opus-4-8");
        assert!((p.cost(1_000_000, 0) - 15.00).abs() < 1e-6);
    }
    #[test]
    fn unknown_model_falls_back_to_fable_pricing() {
        let a = ModelPricing::for_model("unknown-xyz").cost(100_000, 100_000);
        let b = ModelPricing::for_model("claude-fable-5").cost(100_000, 100_000);
        assert!((a - b).abs() < 1e-9);
    }
    #[test]
    fn zero_tokens_costs_zero() {
        assert_eq!(ModelPricing::for_model("claude-fable-5").cost(0, 0), 0.0);
    }
    #[test]
    fn output_tokens_more_expensive_than_input() {
        let p = ModelPricing::for_model("claude-fable-5");
        assert!(p.cost(0, 1_000_000) > p.cost(1_000_000, 0));
    }
    #[test]
    fn simple_routes_to_haiku() {
        let c = Classification { complexity: Complexity::Simple, task_type: TaskType::Other, estimated_output_tokens: 100, reasoning: "t".into(), routed_to: String::new(), cache_hit: false };
        assert_eq!(c.route_model(), HAIKU);
    }
    #[test]
    fn medium_routes_to_sonnet() {
        let c = Classification { complexity: Complexity::Medium, task_type: TaskType::Code, estimated_output_tokens: 512, reasoning: "t".into(), routed_to: String::new(), cache_hit: false };
        assert_eq!(c.route_model(), SONNET);
    }
    #[test]
    fn complex_routes_to_fable() {
        let c = Classification { complexity: Complexity::Complex, task_type: TaskType::Research, estimated_output_tokens: 2048, reasoning: "t".into(), routed_to: String::new(), cache_hit: false };
        assert_eq!(c.route_model(), FABLE);
    }
    #[test]
    fn error_response_builds_correctly() {
        let e = ErrorResponse::new("Something went wrong", "test_error");
        assert_eq!(e.error.message, "Something went wrong");
        assert_eq!(e.error.code, "test_error");
    }
    #[test]
    fn default_guardrails_are_sensible() {
        let g = GuardrailsConfig::default();
        assert!(g.guardrails.max_cost_per_request_usd > 0.0);
        assert!(!g.guardrails.blocked_phrases.is_empty());
    }
    #[test]
    fn model_budgets_default_disabled() {
        let g = GuardrailsConfig::default();
        assert!(!g.model_budgets.enabled);
    }
    #[test]
    fn model_budgets_has_limit_for_fable() {
        let g = GuardrailsConfig::default();
        assert!(g.model_budgets.limit_for(FABLE).is_some());
    }
    #[test]
    fn model_budgets_fallback_is_default_action() {
        let g = GuardrailsConfig::default();
        assert!(g.model_budgets.should_fallback_on_exceed());
    }
}
