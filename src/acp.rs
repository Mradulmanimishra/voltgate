use axum::{extract::State, Json};
use chrono::Utc;
use uuid::Uuid;

use crate::models::{AcpRequest, AcpResponse, AnthropicRequest, OpenAIMessage, ModelPricing, ApiCallRecord};
use crate::database::insert_call;
use crate::state::AppState;

pub async fn handle_acp(State(state): State<AppState>, Json(req): Json<AcpRequest>) -> Json<AcpResponse> {
    let task = req.task.trim().to_string();
    let messages = vec![OpenAIMessage { role: "user".to_string(), content: serde_json::Value::String(task.clone()) }];
    let mut classification = state.classifier.classify(&messages).await;

    let model = req.model_hint.clone().unwrap_or_else(|| classification.route_model().to_string());
    classification.routed_to = model.clone();

    let anthropic_req = AnthropicRequest {
        model: model.clone(), messages, max_tokens: 2048, temperature: Some(0.7),
        system: Some("You are a helpful AI agent executing a delegated task. Complete the task efficiently and return a clear result.".to_string()),
    };

    let start  = std::time::Instant::now();
    let result = match state.classifier.call_anthropic(&anthropic_req).await {
        Ok(resp) => resp.content.iter().filter(|b| b.block_type == "text").filter_map(|b| b.text.as_ref()).cloned().collect::<Vec<_>>().join(""),
        Err(e)   => format!("ACP task failed: {e}"),
    };

    let latency_ms = start.elapsed().as_millis() as i64;
    let pricing    = ModelPricing::for_model(&model);
    let out_tokens = (result.len() / 4) as i64;
    let in_tokens  = (task.len() / 4) as i64;
    let cost_usd   = pricing.cost(in_tokens, out_tokens);

    let record = ApiCallRecord {
        request_id: Uuid::new_v4().to_string(), routed_model: model.clone(), task_type: "acp".to_string(),
        complexity: format!("{:?}", classification.complexity).to_lowercase(),
        input_tokens: in_tokens, output_tokens: out_tokens, cost_usd, latency_ms,
        cache_hit: false, timestamp: Utc::now(),
    };
    if let Err(e) = insert_call(&state.db, &record) { tracing::warn!("ACP DB insert failed: {e}"); }

    let budget_remaining = req.budget_usd.map(|b| (b - cost_usd).max(0.0));
    Json(AcpResponse { result, model_used: model, cost_usd, budget_remaining })
}
