/// fallback.rs — model fallback chain.
/// fable-5 → sonnet-4-6 → haiku-4-5 (haiku has no further fallback)
/// opus-4-8 → sonnet-4-6 → haiku-4-5
/// sonnet-4-6 → haiku-4-5

use crate::models::{HAIKU, SONNET};

pub fn next_fallback(current_model: &str) -> Option<&'static str> {
    match current_model {
        m if m.contains("fable")  => Some(SONNET),
        m if m.contains("opus")   => Some(SONNET),
        m if m.contains("sonnet") => Some(HAIKU),
        m if m.contains("haiku")  => None,
        _                         => Some(SONNET),
    }
}

pub fn fallback_chain(starting_model: &str) -> Vec<String> {
    let mut chain = vec![starting_model.to_string()];
    let mut current = starting_model.to_string();
    for _ in 0..3 {
        match next_fallback(&current) {
            Some(next) if !chain.contains(&next.to_string()) => {
                chain.push(next.to_string());
                current = next.to_string();
            }
            _ => break,
        }
    }
    chain
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FallbackOutcome {
    pub final_model:    String,
    pub original_model: String,
    pub fallback_used:  bool,
    pub hops:           u32,
}

impl FallbackOutcome {
    pub fn no_fallback(model: &str) -> Self {
        Self { final_model: model.to_string(), original_model: model.to_string(), fallback_used: false, hops: 0 }
    }
    pub fn fell_back_to(original: &str, final_model: &str, hops: u32) -> Self {
        Self { final_model: final_model.to_string(), original_model: original.to_string(), fallback_used: true, hops }
    }
}
