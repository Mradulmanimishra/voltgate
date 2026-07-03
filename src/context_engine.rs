/// context_engine.rs — Context Engineering pipeline.
/// Runs BEFORE every Anthropic call: compress, trim, cache, inject.

use crate::models::{
    AnthropicRequest, OpenAIMessage, TaskType, Complexity,
    Classification, AnthropicResponse, HAIKU, ANTHROPIC_API,
};

const CHARS_PER_TOKEN: usize = 4;
const SIMPLE_MAX_INPUT_TOKENS:  usize = 4_000;
const MEDIUM_MAX_INPUT_TOKENS:  usize = 40_000;
const COMPLEX_MAX_INPUT_TOKENS: usize = 180_000;
const COMPRESS_THRESHOLD_TOKENS: usize = 8_000;
const KEEP_RECENT_MESSAGES: usize = 6;

fn system_prompt_for(task_type: &TaskType) -> &'static str {
    match task_type {
        TaskType::Code => "You are an expert software engineer. Write clean, idiomatic, well-commented code. Always explain your reasoning. Prefer correctness over brevity. Highlight edge cases.",
        TaskType::Research => "You are a rigorous research analyst. Provide accurate, well-sourced answers. Distinguish between facts and inferences. Acknowledge uncertainty explicitly. Cite evidence.",
        TaskType::Creative => "You are a skilled creative writer. Produce vivid, engaging content with strong narrative voice. Vary sentence structure. Show, don't tell.",
        TaskType::Data => "You are a data scientist and analyst. Reason carefully about numbers, statistics, and patterns. Show your working. Flag any assumptions about the data.",
        TaskType::Other => "You are a helpful, precise, and honest assistant. Answer clearly and concisely. If unsure, say so.",
    }
}

/// Estimate token count for a string using a 4-chars/token heuristic.
/// Public API — used by external callers wanting to pre-check payload
/// size, and exercised directly by tests/test_context_engine.rs.
#[allow(dead_code)]
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(CHARS_PER_TOKEN)
}

pub fn estimate_messages_tokens(messages: &[OpenAIMessage]) -> usize {
    messages.iter().map(|m| {
        let content_len = match &m.content {
            serde_json::Value::String(s) => s.len(),
            serde_json::Value::Array(arr) => arr.iter()
                .filter_map(|v| v.get("text")?.as_str())
                .map(|s| s.len()).sum(),
            _ => 0,
        };
        content_len.div_ceil(CHARS_PER_TOKEN) + 4
    }).sum()
}

pub fn input_budget_for(complexity: &Complexity) -> usize {
    match complexity {
        Complexity::Simple  => SIMPLE_MAX_INPUT_TOKENS,
        Complexity::Medium  => MEDIUM_MAX_INPUT_TOKENS,
        Complexity::Complex => COMPLEX_MAX_INPUT_TOKENS,
    }
}

pub fn trim_to_budget(messages: &[OpenAIMessage], budget: usize) -> Vec<OpenAIMessage> {
    let mut result = messages.to_vec();
    while result.len() > 1 && estimate_messages_tokens(&result) > budget {
        result.remove(0);
    }
    result
}

pub async fn compress_conversation(
    messages: &[OpenAIMessage], api_key: &str, client: &reqwest::Client,
) -> Vec<OpenAIMessage> {
    let total_tokens = estimate_messages_tokens(messages);
    if total_tokens <= COMPRESS_THRESHOLD_TOKENS || messages.len() <= KEEP_RECENT_MESSAGES {
        return messages.to_vec();
    }

    let split_at = messages.len().saturating_sub(KEEP_RECENT_MESSAGES);
    let old_msgs = &messages[..split_at];
    let recent   = &messages[split_at..];

    let transcript: String = old_msgs.iter().map(|m| {
        let text = match &m.content { serde_json::Value::String(s) => s.clone(), v => v.to_string() };
        format!("[{}]: {}\n", m.role.to_uppercase(), text)
    }).collect();

    let summary_prompt = format!(
        "Summarise this conversation in 3-5 concise bullet points. Preserve all key decisions, facts, code snippets, and unresolved questions. Be dense and precise.\n\nCONVERSATION:\n{transcript}"
    );

    let req = AnthropicRequest {
        model: HAIKU.to_string(),
        messages: vec![OpenAIMessage { role: "user".to_string(), content: serde_json::Value::String(summary_prompt) }],
        max_tokens: 512,
        temperature: Some(0.0),
        system: Some("You are a conversation summariser. Output only the bullet-point summary. No preamble.".to_string()),
    };

    let summary = match client.post(ANTHROPIC_API)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&req).send().await
    {
        Ok(resp) => {
            if let Ok(parsed) = resp.json::<AnthropicResponse>().await {
                parsed.content.iter().filter(|b| b.block_type == "text")
                    .filter_map(|b| b.text.as_ref()).cloned().collect::<Vec<_>>().join("")
            } else { return messages.to_vec(); }
        }
        Err(_) => return messages.to_vec(),
    };

    tracing::info!(original_tokens = total_tokens, "Compressed conversation with Haiku");

    let mut compressed = vec![
        OpenAIMessage { role: "user".to_string(), content: serde_json::Value::String(format!("[CONVERSATION SUMMARY — earlier context]\n{summary}")) },
        OpenAIMessage { role: "assistant".to_string(), content: serde_json::Value::String("Understood. I have the context from the earlier conversation.".to_string()) },
    ];
    compressed.extend_from_slice(recent);
    compressed
}

pub fn build_cached_request(
    messages: Vec<OpenAIMessage>, model: String, max_tokens: i64,
    temperature: Option<f64>, task_type: &TaskType, user_system: Option<String>,
) -> serde_json::Value {
    let base_system = system_prompt_for(task_type);
    let system_text = match user_system {
        Some(user) => format!("{base_system}\n\n{user}"),
        None => base_system.to_string(),
    };

    let messages_json: Vec<serde_json::Value> = messages.iter().enumerate().map(|(i, m)| {
        let mut msg = serde_json::json!({ "role": m.role });
        let content_str = match &m.content { serde_json::Value::String(s) => s.clone(), other => other.to_string() };
        if i == 0 && m.role == "user" && content_str.len() > 4096 {
            msg["content"] = serde_json::json!([{ "type": "text", "text": content_str, "cache_control": { "type": "ephemeral" } }]);
        } else {
            msg["content"] = m.content.clone();
        }
        msg
    }).collect();

    serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "temperature": temperature.unwrap_or(0.7),
        "system": [{ "type": "text", "text": system_text, "cache_control": { "type": "ephemeral" } }],
        "messages": messages_json
    })
}

pub fn optimise_max_tokens(requested: Option<i64>, complexity: &Complexity) -> i64 {
    let tier_cap = match complexity {
        Complexity::Simple  => 512_i64,
        Complexity::Medium  => 4_096,
        Complexity::Complex => 16_384,
    };
    match requested { Some(r) => r.min(tier_cap), None => tier_cap / 2 }
}

pub async fn prepare_request(
    messages: &[OpenAIMessage], max_tokens: Option<i64>, temperature: Option<f64>,
    classification: &Classification, model: &str, api_key: &str,
    client: &reqwest::Client, user_system: Option<String>,
) -> (serde_json::Value, ContextReport) {
    let original_tokens = estimate_messages_tokens(messages);
    let compressed = compress_conversation(messages, api_key, client).await;
    let compressed_tokens = estimate_messages_tokens(&compressed);
    let budget = input_budget_for(&classification.complexity);
    let trimmed = trim_to_budget(&compressed, budget);
    let final_tokens = estimate_messages_tokens(&trimmed);
    let optimised_max = optimise_max_tokens(max_tokens, &classification.complexity);
    let body = build_cached_request(trimmed, model.to_string(), optimised_max, temperature, &classification.task_type, user_system);

    let report = ContextReport {
        original_tokens, compressed_tokens, final_tokens,
        optimised_max_tokens: optimised_max,
        budget_used_pct: (final_tokens as f64 / budget as f64 * 100.0).min(100.0),
        compression_applied: compressed_tokens < original_tokens,
        trim_applied: final_tokens < compressed_tokens,
    };

    tracing::debug!(original = original_tokens, compressed = compressed_tokens, final = final_tokens, max_tokens = optimised_max, "Context engineering complete");
    (body, report)
}

#[derive(Debug, serde::Serialize, Clone)]
pub struct ContextReport {
    pub original_tokens:      usize,
    pub compressed_tokens:    usize,
    pub final_tokens:         usize,
    pub optimised_max_tokens: i64,
    pub budget_used_pct:      f64,
    pub compression_applied:  bool,
    pub trim_applied:         bool,
}
