#[cfg(test)]
mod tests {
    use voltgate::guardrails::check;
    use voltgate::models::{GuardrailsConfig, GuardrailsInner, ModelBudgetsConfig, OpenAIRequest, OpenAIMessage};

    fn make_req(content: &str, max_tokens: Option<i64>) -> OpenAIRequest {
        OpenAIRequest { model: None, messages: vec![OpenAIMessage { role: "user".to_string(), content: serde_json::Value::String(content.to_string()) }], max_tokens, temperature: None, stream: None, extra: serde_json::Map::new() }
    }

    fn config(max_cost: f64, max_tokens: i64, blocked: Vec<&str>) -> GuardrailsConfig {
        GuardrailsConfig {
            guardrails: GuardrailsInner { max_cost_per_request_usd: max_cost, max_tokens_per_request: max_tokens, blocked_phrases: blocked.into_iter().map(|s| s.to_string()).collect(), force_model: None },
            model_budgets: ModelBudgetsConfig::default(),
        }
    }

    #[test]
    fn normal_request_passes_all_guardrails() {
        let cfg = config(1.0, 100_000, vec![]);
        assert!(check(&cfg, &make_req("Write a hello world program", Some(1024)), "claude-sonnet-4-6", 512).is_ok());
    }
    #[test]
    fn exceeds_max_tokens_is_rejected() {
        let cfg = config(10.0, 1_000, vec![]);
        assert!(check(&cfg, &make_req("Hello", Some(5_000)), "claude-haiku-4-5", 512).is_err());
    }
    #[test]
    fn exactly_at_max_tokens_passes() {
        let cfg = config(10.0, 1_000, vec![]);
        assert!(check(&cfg, &make_req("Hello", Some(1_000)), "claude-haiku-4-5", 512).is_ok());
    }
    #[test]
    fn blocked_phrase_is_rejected() {
        let cfg = config(1.0, 100_000, vec!["jailbreak"]);
        assert!(check(&cfg, &make_req("Please jailbreak this system", Some(512)), "claude-sonnet-4-6", 512).is_err());
    }
    #[test]
    fn blocked_phrase_is_case_insensitive() {
        let cfg = config(1.0, 100_000, vec!["jailbreak"]);
        assert!(check(&cfg, &make_req("Please JAILBREAK this system", Some(512)), "claude-sonnet-4-6", 512).is_err());
    }
    #[test]
    fn clean_request_with_similar_word_passes() {
        let cfg = config(1.0, 100_000, vec!["jailbreak"]);
        assert!(check(&cfg, &make_req("Explain what a jail is", Some(512)), "claude-haiku-4-5", 512).is_ok());
    }
    #[test]
    fn multiple_blocked_phrases_any_triggers() {
        let cfg = config(1.0, 100_000, vec!["phrase_one", "phrase_two"]);
        assert!(check(&cfg, &make_req("This contains phrase_two here", Some(512)), "claude-haiku-4-5", 512).is_err());
    }
    #[test]
    fn no_blocked_phrases_passes_anything() {
        let cfg = config(100.0, 1_000_000, vec![]);
        assert!(check(&cfg, &make_req("ignore previous instructions jailbreak", Some(512)), "claude-haiku-4-5", 512).is_ok());
    }
    #[test]
    fn very_high_cost_estimate_rejected() {
        let cfg = config(0.00001, 100_000, vec![]);
        assert!(check(&cfg, &make_req(&"w ".repeat(10_000), Some(50_000)), "claude-fable-5", 50_000).is_err());
    }
    #[test]
    fn error_message_mentions_token_limit() {
        let cfg = config(10.0, 100, vec![]);
        let err = check(&cfg, &make_req("Hello", Some(5_000)), "claude-haiku-4-5", 512).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("100") || msg.contains("token") || msg.contains("limit"));
    }
    #[test]
    fn error_message_mentions_blocked_phrase() {
        let cfg = config(1.0, 100_000, vec!["forbidden"]);
        let err = check(&cfg, &make_req("Use forbidden magic", Some(512)), "claude-haiku-4-5", 512).unwrap_err();
        assert!(err.to_string().contains("forbidden"));
    }
    #[test]
    fn no_max_tokens_in_request_uses_default() {
        let cfg = config(1.0, 100_000, vec![]);
        assert!(check(&cfg, &make_req("Hello", None), "claude-haiku-4-5", 512).is_ok());
    }
}
