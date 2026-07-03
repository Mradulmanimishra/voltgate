#[cfg(test)]
mod tests {
    use voltgate::fallback::{next_fallback, fallback_chain, FallbackOutcome};
    use voltgate::models::{HAIKU, SONNET, FABLE, OPUS};

    #[test]
    fn fable_falls_back_to_sonnet() {
        assert_eq!(next_fallback(FABLE), Some(SONNET));
    }
    #[test]
    fn opus_falls_back_to_sonnet() {
        assert_eq!(next_fallback(OPUS), Some(SONNET));
    }
    #[test]
    fn sonnet_falls_back_to_haiku() {
        assert_eq!(next_fallback(SONNET), Some(HAIKU));
    }
    #[test]
    fn haiku_has_no_fallback() {
        assert_eq!(next_fallback(HAIKU), None);
    }
    #[test]
    fn unknown_model_falls_back_to_sonnet_as_safe_default() {
        assert_eq!(next_fallback("some-unknown-model"), Some(SONNET));
    }

    #[test]
    fn fable_chain_has_three_models() {
        let chain = fallback_chain(FABLE);
        assert_eq!(chain, vec![FABLE.to_string(), SONNET.to_string(), HAIKU.to_string()]);
    }
    #[test]
    fn opus_chain_has_three_models() {
        let chain = fallback_chain(OPUS);
        assert_eq!(chain, vec![OPUS.to_string(), SONNET.to_string(), HAIKU.to_string()]);
    }
    #[test]
    fn sonnet_chain_has_two_models() {
        let chain = fallback_chain(SONNET);
        assert_eq!(chain, vec![SONNET.to_string(), HAIKU.to_string()]);
    }
    #[test]
    fn haiku_chain_has_only_itself() {
        let chain = fallback_chain(HAIKU);
        assert_eq!(chain, vec![HAIKU.to_string()]);
    }
    #[test]
    fn chain_never_exceeds_four_hops() {
        // Guard against accidental infinite loops / cycles
        for model in [FABLE, OPUS, SONNET, HAIKU, "unknown-model"] {
            assert!(fallback_chain(model).len() <= 4, "chain for {model} too long");
        }
    }
    #[test]
    fn chain_has_no_duplicate_models() {
        for model in [FABLE, OPUS, SONNET, HAIKU] {
            let chain = fallback_chain(model);
            let mut sorted = chain.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(chain.len(), sorted.len(), "chain for {model} has duplicates: {chain:?}");
        }
    }
    #[test]
    fn chain_first_element_is_starting_model() {
        assert_eq!(fallback_chain(FABLE)[0], FABLE);
        assert_eq!(fallback_chain(SONNET)[0], SONNET);
    }

    #[test]
    fn no_fallback_outcome_reports_correctly() {
        let outcome = FallbackOutcome::no_fallback(FABLE);
        assert!(!outcome.fallback_used);
        assert_eq!(outcome.final_model, FABLE);
        assert_eq!(outcome.original_model, FABLE);
        assert_eq!(outcome.hops, 0);
    }
    #[test]
    fn fell_back_outcome_reports_correctly() {
        let outcome = FallbackOutcome::fell_back_to(FABLE, SONNET, 1);
        assert!(outcome.fallback_used);
        assert_eq!(outcome.original_model, FABLE);
        assert_eq!(outcome.final_model, SONNET);
        assert_eq!(outcome.hops, 1);
    }
    #[test]
    fn partial_model_string_matches_correctly() {
        // Real Anthropic model strings sometimes include date suffixes,
        // e.g. "claude-fable-5-20260609" — contains() matching must still work.
        assert_eq!(next_fallback("claude-fable-5-20260609"), Some(SONNET));
    }
}
