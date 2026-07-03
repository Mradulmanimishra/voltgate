/// proxy.rs — Forwards requests to Anthropic.
/// Full pipeline: context_engine → retry (exp backoff) → fallback chain
/// → parse response → log to SQLite → record metrics.

use std::time::Instant;
use chrono::Utc;
use uuid::Uuid;

use crate::models::{
    AnthropicResponse, OpenAIRequest, OpenAIResponse, OpenAIChoice,
    OpenAIMessage, OpenAIUsage, RouterMeta, ModelPricing, ApiCallRecord,
    Classification, ANTHROPIC_API,
};
use crate::database::{Db, insert_call};
use crate::context_engine::{self, ContextReport};
use crate::metrics::METRICS;
use crate::retry::with_retry;
use crate::fallback::{fallback_chain, FallbackOutcome};

#[derive(Clone)]
pub struct Proxy {
    pub api_key: String,
    pub client:  reqwest::Client,
}

impl Proxy {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::builder().timeout(std::time::Duration::from_secs(120)).build().unwrap(),
        }
    }

    /// Full pipeline. Tries the routed model first; on repeated
    /// transient failure (529/503/etc after MAX_ATTEMPTS retries)
    /// falls back to the next model in the chain rather than failing
    /// the whole request.
    pub async fn forward(
        &self, req: &OpenAIRequest, routed_model: &str,
        classification: &Classification, db: &Db,
    ) -> Result<(OpenAIResponse, ContextReport, FallbackOutcome), String> {
        let (user_system, user_messages) = split_system(req);

        let (ce_body, ce_report) = context_engine::prepare_request(
            &user_messages, req.max_tokens, req.temperature, classification,
            routed_model, &self.api_key, &self.client, user_system,
        ).await;

        let chain = fallback_chain(routed_model);
        let mut last_error = String::new();

        for (hop, candidate_model) in chain.iter().enumerate() {
            let mut body = ce_body.clone();
            body["model"] = serde_json::json!(candidate_model);

            METRICS.request_started();
            let start      = Instant::now();
            let request_id = Uuid::new_v4().to_string();

            let outcome = with_retry(|_attempt| {
                let client = self.client.clone();
                let api_key = self.api_key.clone();
                let body = body.clone();
                async move {
                    let http_resp = client.post(ANTHROPIC_API)
                        .header("x-api-key", &api_key)
                        .header("anthropic-version", "2023-06-01")
                        .header("anthropic-beta", "prompt-caching-2024-07-31")
                        .header("content-type", "application/json")
                        .json(&body).send().await
                        .map_err(|e| (0u16, format!("Network error: {e}")))?;

                    let status = http_resp.status();
                    if !status.is_success() {
                        let text = http_resp.text().await.unwrap_or_default();
                        return Err((status.as_u16(), format!("Anthropic {status}: {text}")));
                    }
                    http_resp.json::<AnthropicResponse>().await
                        .map_err(|e| (0u16, format!("Parse error: {e}")))
                }
            }).await;

            if outcome.retried { METRICS.retry_occurred(); }

            match outcome.result {
                Ok(anthropic_resp) => {
                    let latency_ms = start.elapsed().as_millis() as i64;
                    let pricing    = ModelPricing::for_model(candidate_model);
                    let cost_usd   = pricing.cost(anthropic_resp.usage.input_tokens, anthropic_resp.usage.output_tokens);

                    let record = ApiCallRecord {
                        request_id: request_id.clone(), routed_model: candidate_model.clone(),
                        task_type: format!("{:?}", classification.task_type).to_lowercase(),
                        complexity: format!("{:?}", classification.complexity).to_lowercase(),
                        input_tokens: anthropic_resp.usage.input_tokens,
                        output_tokens: anthropic_resp.usage.output_tokens,
                        cost_usd, latency_ms, cache_hit: classification.cache_hit, timestamp: Utc::now(),
                    };
                    if let Err(e) = insert_call(db, &record) { tracing::warn!("DB insert failed: {e}"); }

                    let fallback_outcome = if hop == 0 {
                        FallbackOutcome::no_fallback(routed_model)
                    } else {
                        METRICS.fallback_occurred();
                        FallbackOutcome::fell_back_to(routed_model, candidate_model, hop as u32)
                    };

                    METRICS.request_finished(candidate_model,
                        &format!("{:?}", classification.complexity).to_lowercase(),
                        &format!("{:?}", classification.task_type).to_lowercase(),
                        "success", cost_usd, latency_ms as f64, ce_report.final_tokens,
                        classification.cache_hit, ce_report.compression_applied);

                    let response_text = anthropic_resp.content.iter()
                        .filter(|b| b.block_type == "text")
                        .filter_map(|b| b.text.as_ref()).cloned().collect::<Vec<_>>().join("");

                    let oai_resp = OpenAIResponse {
                        id: format!("chatcmpl-{request_id}"), object: "chat.completion".to_string(),
                        created: Utc::now().timestamp(), model: candidate_model.clone(),
                        choices: vec![OpenAIChoice {
                            index: 0,
                            message: OpenAIMessage { role: "assistant".to_string(), content: serde_json::Value::String(response_text) },
                            finish_reason: anthropic_resp.stop_reason,
                        }],
                        usage: OpenAIUsage {
                            prompt_tokens: anthropic_resp.usage.input_tokens,
                            completion_tokens: anthropic_resp.usage.output_tokens,
                            total_tokens: anthropic_resp.usage.input_tokens + anthropic_resp.usage.output_tokens,
                        },
                        x_router: Some(RouterMeta {
                            routed_to: candidate_model.clone(),
                            complexity: format!("{:?}", classification.complexity).to_lowercase(),
                            task_type: format!("{:?}", classification.task_type).to_lowercase(),
                            estimated_output_tokens: classification.estimated_output_tokens,
                            cost_usd, cache_hit: classification.cache_hit,
                            reasoning: classification.reasoning.clone(),
                            fallback_used: Some(fallback_outcome.fallback_used),
                            original_model: Some(fallback_outcome.original_model.clone()),
                            retry_attempts: Some(outcome.attempts),
                        }),
                    };

                    return Ok((oai_resp, ce_report, fallback_outcome));
                }
                Err(msg) => {
                    tracing::warn!(model = candidate_model, attempts = outcome.attempts, "Exhausted retries, trying next fallback: {msg}");
                    last_error = msg;
                    METRICS.request_finished(candidate_model,
                        &format!("{:?}", classification.complexity).to_lowercase(),
                        &format!("{:?}", classification.task_type).to_lowercase(),
                        "error", 0.0, start.elapsed().as_millis() as f64,
                        ce_report.final_tokens, classification.cache_hit, ce_report.compression_applied);
                    continue;
                }
            }
        }

        Err(format!("All models in fallback chain exhausted. Last error: {last_error}"))
    }
}

fn split_system(req: &OpenAIRequest) -> (Option<String>, Vec<OpenAIMessage>) {
    let mut system: Option<String> = None;
    let messages = req.messages.iter().filter_map(|m| {
        if m.role == "system" {
            system = Some(match &m.content { serde_json::Value::String(s) => s.clone(), other => other.to_string() });
            None
        } else { Some(m.clone()) }
    }).collect();
    (system, messages)
}
