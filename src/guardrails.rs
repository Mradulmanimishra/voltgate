use crate::models::{GuardrailsConfig, OpenAIRequest, ModelPricing};
use crate::database::Db;

#[derive(Debug)]
pub enum GuardrailViolation {
    CostTooHigh { estimated: f64, limit: f64 },
    TokensTooMany { requested: i64, limit: i64 },
    BlockedPhrase { phrase: String },
    ModelDailyBudgetExceeded { model: String, spent: f64, limit: f64 },
}

impl std::fmt::Display for GuardrailViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CostTooHigh { estimated, limit } =>
                write!(f, "Estimated cost ${estimated:.4} exceeds limit ${limit:.4}"),
            Self::TokensTooMany { requested, limit } =>
                write!(f, "max_tokens {requested} exceeds limit {limit}"),
            Self::BlockedPhrase { phrase } =>
                write!(f, "Request contains blocked phrase: '{phrase}'"),
            Self::ModelDailyBudgetExceeded { model, spent, limit } =>
                write!(f, "Model '{model}' daily budget exceeded: ${spent:.2} of ${limit:.2}"),
        }
    }
}

pub fn check(
    config: &GuardrailsConfig, req: &OpenAIRequest, routed_model: &str, est_output: i64,
) -> Result<(), GuardrailViolation> {
    let g = &config.guardrails;

    let max_t = req.max_tokens.unwrap_or(4096);
    if max_t > g.max_tokens_per_request {
        return Err(GuardrailViolation::TokensTooMany { requested: max_t, limit: g.max_tokens_per_request });
    }

    let pricing   = ModelPricing::for_model(routed_model);
    let est_input = extract_approx_input_tokens(req);
    let est_cost  = pricing.cost(est_input, est_output.min(max_t));
    if est_cost > g.max_cost_per_request_usd {
        return Err(GuardrailViolation::CostTooHigh { estimated: est_cost, limit: g.max_cost_per_request_usd });
    }

    let prompt_text = extract_all_text(req);
    let lower = prompt_text.to_lowercase();
    for phrase in &g.blocked_phrases {
        if lower.contains(&phrase.to_lowercase()) {
            return Err(GuardrailViolation::BlockedPhrase { phrase: phrase.clone() });
        }
    }

    Ok(())
}

/// Check per-model daily spend budget. Called separately from `check()`
/// because it needs DB access. Returns Ok(true) if fallback should be
/// attempted instead of rejecting outright.
pub fn check_model_budget(
    config: &GuardrailsConfig, db: &Db, model: &str,
) -> Result<(), GuardrailViolation> {
    if !config.model_budgets.enabled {
        return Ok(());
    }
    let Some(limit) = config.model_budgets.limit_for(model) else { return Ok(()) };
    let spent = crate::database::model_spend_today(db, model);
    if spent >= limit {
        return Err(GuardrailViolation::ModelDailyBudgetExceeded {
            model: model.to_string(), spent, limit,
        });
    }
    Ok(())
}

fn extract_approx_input_tokens(req: &OpenAIRequest) -> i64 {
    (extract_all_text(req).len() as i64) / 4
}

fn extract_all_text(req: &OpenAIRequest) -> String {
    req.messages.iter().map(|m| match &m.content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr.iter()
            .filter_map(|v| {
                if v.get("type").and_then(|t| t.as_str()) == Some("text") {
                    v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                } else { None }
            }).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    }).collect::<Vec<_>>().join(" ")
}
