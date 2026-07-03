#[cfg(test)]
mod tests {
    use voltgate::context_engine::*;
    use voltgate::models::{OpenAIMessage, Complexity, TaskType};

    fn make_msg(role: &str, content: &str) -> OpenAIMessage {
        OpenAIMessage { role: role.to_string(), content: serde_json::Value::String(content.to_string()) }
    }

    #[test]
    fn empty_string_is_zero_tokens() { assert_eq!(estimate_tokens(""), 0); }
    #[test]
    fn four_chars_is_one_token() { assert_eq!(estimate_tokens("test"), 1); }
    #[test]
    fn token_estimate_rounds_up() { assert_eq!(estimate_tokens("hello"), 2); }
    #[test]
    fn empty_messages_is_zero() { assert_eq!(estimate_messages_tokens(&[]), 0); }
    #[test]
    fn single_message_estimate_includes_overhead() {
        let msgs = vec![make_msg("user", "Hi")];
        assert!(estimate_messages_tokens(&msgs) >= 1);
    }
    #[test]
    fn longer_conversation_has_more_tokens() {
        let short = vec![make_msg("user", "Hi")];
        let long = vec![
            make_msg("user", "Hello, I need help with a complex Rust program"),
            make_msg("assistant", "Of course! Please share the code and describe the issue."),
            make_msg("user", "Here it is: fn main() { ... }"),
        ];
        assert!(estimate_messages_tokens(&long) > estimate_messages_tokens(&short));
    }
    #[test]
    fn simple_budget_is_smallest() {
        assert!(input_budget_for(&Complexity::Simple) < input_budget_for(&Complexity::Medium));
        assert!(input_budget_for(&Complexity::Medium) < input_budget_for(&Complexity::Complex));
    }
    #[test]
    fn simple_budget_under_10k() { assert!(input_budget_for(&Complexity::Simple) <= 10_000); }
    #[test]
    fn complex_budget_allows_long_context() { assert!(input_budget_for(&Complexity::Complex) >= 100_000); }
    #[test]
    fn trim_keeps_at_least_one_message() {
        let msgs = vec![make_msg("user", "Hello")];
        assert_eq!(trim_to_budget(&msgs, 0).len(), 1);
    }
    #[test]
    fn trim_does_nothing_when_under_budget() {
        let msgs = vec![make_msg("user", "Short"), make_msg("assistant", "Short reply")];
        assert_eq!(trim_to_budget(&msgs, 100_000).len(), msgs.len());
    }
    #[test]
    fn trim_reduces_length_when_over_budget() {
        let msgs: Vec<OpenAIMessage> = (0..20).map(|i| make_msg("user", &"word ".repeat(500 + i))).collect();
        let original = msgs.len();
        assert!(trim_to_budget(&msgs, 100).len() < original);
    }
    #[test]
    fn trim_keeps_most_recent_messages() {
        let msgs = vec![make_msg("user", "Old message one"), make_msg("user", "Old message two"), make_msg("user", "RECENT MESSAGE")];
        let trimmed = trim_to_budget(&msgs, 20);
        let last = trimmed.last().unwrap();
        let content = match &last.content { serde_json::Value::String(s) => s.clone(), _ => String::new() };
        assert!(content.contains("RECENT"));
    }
    #[test]
    fn simple_caps_output_at_512() { assert!(optimise_max_tokens(Some(10_000), &Complexity::Simple) <= 512); }
    #[test]
    fn complex_allows_large_output() { assert!(optimise_max_tokens(Some(10_000), &Complexity::Complex) >= 4_096); }
    #[test]
    fn respects_caller_limit_if_lower() { assert_eq!(optimise_max_tokens(Some(100), &Complexity::Complex), 100); }
    #[test]
    fn none_max_tokens_uses_default() { assert!(optimise_max_tokens(None, &Complexity::Medium) > 0); }
    #[test]
    fn cached_request_has_system_field() {
        let body = build_cached_request(vec![make_msg("user", "Hello")], "claude-sonnet-4-6".into(), 512, None, &TaskType::Code, None);
        assert!(body.get("system").is_some());
    }
    #[test]
    fn cached_request_injects_system_prompt() {
        let body = build_cached_request(vec![make_msg("user", "Write a function")], "claude-sonnet-4-6".into(), 512, None, &TaskType::Code, None);
        let sys = body["system"].to_string();
        assert!(sys.contains("engineer") || sys.contains("code"));
    }
    #[test]
    fn cached_request_merges_user_system_prompt() {
        let body = build_cached_request(vec![make_msg("user", "Hello")], "claude-sonnet-4-6".into(), 512, None, &TaskType::Other, Some("You must reply in French.".to_string()));
        assert!(body["system"].to_string().contains("French"));
    }
    #[test]
    fn system_block_has_cache_control() {
        let body = build_cached_request(vec![make_msg("user", "Hello")], "claude-sonnet-4-6".into(), 512, None, &TaskType::Other, None);
        let sys_arr = body["system"].as_array().unwrap();
        assert!(sys_arr.iter().any(|b| b.get("cache_control").and_then(|c| c.get("type")).and_then(|t| t.as_str()) == Some("ephemeral")));
    }
    #[test]
    fn long_first_message_gets_cache_control() {
        let long = "x".repeat(5000);
        let body = build_cached_request(vec![make_msg("user", &long)], "claude-sonnet-4-6".into(), 1024, None, &TaskType::Research, None);
        let messages = body["messages"].as_array().unwrap();
        let has_cache = messages[0]["content"].as_array().map(|a| a.iter().any(|b| b.get("cache_control").is_some())).unwrap_or(false);
        assert!(has_cache);
    }
    #[test]
    fn short_first_message_no_cache_control() {
        let body = build_cached_request(vec![make_msg("user", "Hi")], "claude-sonnet-4-6".into(), 128, None, &TaskType::Other, None);
        assert!(body["messages"].as_array().unwrap()[0]["content"].is_string());
    }
}
