/// streaming.rs — SSE streaming support. Translates Anthropic's SSE
/// event format into OpenAI-compatible streaming chunks so existing
/// client SDKs (openai-python, langchain, etc.) work unmodified.

use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Instant;
use chrono::Utc;
use uuid::Uuid;

use crate::models::{ApiCallRecord, Classification, ModelPricing, ANTHROPIC_API};
use crate::database::{Db, insert_call};
use crate::metrics::METRICS;

pub type SseStream = Sse<std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>>;

pub fn stream_chat_completion(
    anthropic_body: serde_json::Value, api_key: String, routed_model: String,
    classification: Classification, db: Db, client: reqwest::Client,
) -> SseStream {
    let request_id = Uuid::new_v4().to_string();
    let chunk_id   = format!("chatcmpl-{request_id}");

    let stream = async_stream::stream! {
        METRICS.request_started();
        let start = Instant::now();

        let mut body = anthropic_body.clone();
        body["stream"] = serde_json::json!(true);

        let resp = client.post(ANTHROPIC_API)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "prompt-caching-2024-07-31")
            .header("content-type", "application/json")
            .json(&body).send().await;

        let resp = match resp {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                let status = r.status().as_u16();
                let text   = r.text().await.unwrap_or_default();
                tracing::error!(status, "Anthropic streaming request failed: {text}");
                yield Ok(error_event(&format!("Anthropic error {status}: {text}")));
                yield Ok(Event::default().data("[DONE]"));
                return;
            }
            Err(e) => {
                tracing::error!("Network error starting stream: {e}");
                yield Ok(error_event(&format!("Network error: {e}")));
                yield Ok(Event::default().data("[DONE]"));
                return;
            }
        };

        let mut input_tokens:  i64 = 0;
        let mut output_tokens: i64 = 0;
        let mut buffer = String::new();
        let created = Utc::now().timestamp();

        use futures::StreamExt;
        let mut byte_stream = resp.bytes_stream();

        while let Some(chunk_result) = byte_stream.next().await {
            let bytes = match chunk_result {
                Ok(b)  => b,
                Err(e) => { tracing::warn!("Stream read error: {e}"); break; }
            };
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(pos) = buffer.find("\n\n") {
                let raw_event = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                if let Some((event_type, data)) = parse_sse_block(&raw_event) {
                    match event_type.as_str() {
                        "message_start" => {
                            if let Some(tokens) = data.get("message").and_then(|m| m.get("usage")).and_then(|u| u.get("input_tokens")).and_then(|t| t.as_i64()) {
                                input_tokens = tokens;
                            }
                        }
                        "content_block_delta" => {
                            if let Some(text) = data.get("delta").and_then(|d| d.get("text")).and_then(|t| t.as_str()) {
                                let oai_chunk = openai_delta_chunk(&chunk_id, created, &routed_model, text);
                                yield Ok(Event::default().data(serde_json::to_string(&oai_chunk).unwrap()));
                            }
                        }
                        "message_delta" => {
                            if let Some(tokens) = data.get("usage").and_then(|u| u.get("output_tokens")).and_then(|t| t.as_i64()) {
                                output_tokens = tokens;
                            }
                        }
                        "message_stop" => {
                            let final_chunk = openai_final_chunk(&chunk_id, created, &routed_model);
                            yield Ok(Event::default().data(serde_json::to_string(&final_chunk).unwrap()));
                        }
                        "error" => {
                            let msg = data.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("Unknown streaming error");
                            tracing::error!("Anthropic stream error event: {msg}");
                            yield Ok(error_event(msg));
                        }
                        _ => {}
                    }
                }
            }
        }

        yield Ok(Event::default().data("[DONE]"));

        let latency_ms = start.elapsed().as_millis() as i64;
        let pricing    = ModelPricing::for_model(&routed_model);
        let cost_usd   = pricing.cost(input_tokens, output_tokens);

        let record = ApiCallRecord {
            request_id: request_id.clone(), routed_model: routed_model.clone(),
            task_type: format!("{:?}", classification.task_type).to_lowercase(),
            complexity: format!("{:?}", classification.complexity).to_lowercase(),
            input_tokens, output_tokens, cost_usd, latency_ms,
            cache_hit: classification.cache_hit, timestamp: Utc::now(),
        };
        if let Err(e) = insert_call(&db, &record) { tracing::warn!("Streaming DB insert failed: {e}"); }

        METRICS.request_finished(&routed_model,
            &format!("{:?}", classification.complexity).to_lowercase(),
            &format!("{:?}", classification.task_type).to_lowercase(),
            "success_stream", cost_usd, latency_ms as f64,
            (input_tokens + output_tokens) as usize, classification.cache_hit, false);

        tracing::info!(request_id, input_tokens, output_tokens, cost_usd, latency_ms, "Streaming request complete");
    };

    Sse::new(Box::pin(stream))
}

fn parse_sse_block(raw: &str) -> Option<(String, serde_json::Value)> {
    let mut event_type = String::new();
    let mut data_line   = String::new();
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("event: ") { event_type = rest.trim().to_string(); }
        else if let Some(rest) = line.strip_prefix("data: ") { data_line = rest.trim().to_string(); }
    }
    if data_line.is_empty() { return None; }
    let parsed: serde_json::Value = serde_json::from_str(&data_line).ok()?;
    Some((event_type, parsed))
}

fn openai_delta_chunk(id: &str, created: i64, model: &str, text: &str) -> serde_json::Value {
    serde_json::json!({ "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
        "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": null }] })
}

fn openai_final_chunk(id: &str, created: i64, model: &str) -> serde_json::Value {
    serde_json::json!({ "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
        "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] })
}

fn error_event(message: &str) -> Event {
    let payload = serde_json::json!({ "error": { "message": message, "type": "stream_error" } });
    Event::default().data(serde_json::to_string(&payload).unwrap())
}
