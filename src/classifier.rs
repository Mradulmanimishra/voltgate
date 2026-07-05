use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sha2::{Sha256, Digest};

use crate::models::{
    Classification, Complexity, TaskType,
    AnthropicRequest, AnthropicResponse, OpenAIMessage,
    HAIKU, ANTHROPIC_API,
};

const CACHE_TTL_SECS: u64 = 3600;

#[derive(Clone)]
struct CacheEntry {
    classification: Classification,
    inserted_at:    Instant,
}

#[derive(Clone)]
pub struct Classifier {
    api_key: String,
    client:  reqwest::Client,
    cache:   Arc<Mutex<HashMap<String, CacheEntry>>>,
}

impl Classifier {
    pub fn new(api_key: String) -> Self {
        Self { api_key, client: reqwest::Client::new(), cache: Arc::new(Mutex::new(HashMap::new())) }
    }

    fn prompt_hash(text: &str) -> String {
        let mut hasher = Sha256::new();
        // Hash prefix + suffix + length to avoid collisions on templated
        // prompts that share a common preamble but differ in the actual task.
        let prefix = &text[..text.len().min(300)];
        let suffix_start = text.len().saturating_sub(200);
        let suffix = &text[suffix_start..];
        hasher.update(prefix.as_bytes());
        hasher.update(suffix.as_bytes());
        hasher.update(text.len().to_le_bytes());
        hex::encode(hasher.finalize())
    }

    fn get_cached(&self, hash: &str) -> Option<Classification> {
        let mut cache = self.cache.lock().unwrap();
        if let Some(entry) = cache.get(hash) {
            if entry.inserted_at.elapsed() < Duration::from_secs(CACHE_TTL_SECS) {
                let mut c = entry.classification.clone();
                c.cache_hit = true;
                return Some(c);
            }
            cache.remove(hash);
        }
        None
    }

    fn set_cached(&self, hash: String, c: Classification) {
        let mut cache = self.cache.lock().unwrap();
        cache.insert(hash, CacheEntry { classification: c, inserted_at: Instant::now() });
        if cache.len() > 10_000 {
            cache.retain(|_, v| v.inserted_at.elapsed() < Duration::from_secs(CACHE_TTL_SECS));
        }
    }

    fn extract_prompt_text(messages: &[OpenAIMessage]) -> String {
        messages.iter().rev().find_map(|m| {
            match &m.content {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Array(arr) => arr.iter().find_map(|v| {
                    if v.get("type").and_then(|t| t.as_str()) == Some("text") {
                        v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                    } else { None }
                }),
                _ => None,
            }
        }).unwrap_or_default()
    }

    pub async fn classify(&self, messages: &[OpenAIMessage]) -> Classification {
        let prompt_text = Self::extract_prompt_text(messages);
        let hash = Self::prompt_hash(&prompt_text);

        if let Some(cached) = self.get_cached(&hash) {
            return cached;
        }

        let system = "You are a task classifier. Classify the user's request.
Return ONLY valid JSON with exactly these fields:
{
  \"complexity\": \"simple\" | \"medium\" | \"complex\",
  \"task_type\": \"code\" | \"research\" | \"creative\" | \"data\" | \"other\",
  \"estimated_output_tokens\": <integer 1-4096>,
  \"reasoning\": \"<one sentence>\"
}

Guidelines:
- simple:  greetings, single facts, trivial lookups, short questions
- medium:  standard coding tasks, summaries, reviews, analysis, writing
- complex: architecture design, novel algorithms, long-horizon reasoning, multi-step debugging
";

        let snippet = &prompt_text[..prompt_text.len().min(500)];
        let user_msg = format!("Classify this task:\n\n{snippet}");

        let req = AnthropicRequest {
            model: HAIKU.to_string(),
            messages: vec![OpenAIMessage { role: "user".to_string(), content: serde_json::Value::String(user_msg) }],
            max_tokens:  256,
            temperature: Some(0.0),
            system:      Some(system.to_string()),
        };

        match self.call_anthropic(&req).await {
            Ok(resp) => {
                let text = resp.content.iter().find(|b| b.block_type == "text")
                    .and_then(|b| b.text.as_ref()).cloned().unwrap_or_default();
                let parsed = self.parse_classification(&text);
                let routed = parsed.route_model().to_string();
                let mut result = parsed;
                result.routed_to = routed;
                result.cache_hit = false;
                self.set_cached(hash, result.clone());
                result
            }
            Err(e) => {
                tracing::warn!("Classifier failed: {e}. Defaulting to medium/Sonnet.");
                Classification {
                    complexity: Complexity::Medium,
                    task_type:  TaskType::Other,
                    estimated_output_tokens: 512,
                    reasoning:  format!("Classifier error: {e}. Defaulted to medium."),
                    routed_to:  crate::models::SONNET.to_string(),
                    cache_hit:  false,
                }
            }
        }
    }

    fn parse_classification(&self, text: &str) -> Classification {
        let clean = text.replace("```json", "").replace("```", "").trim().to_string();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&clean) {
            let complexity = match v.get("complexity").and_then(|c| c.as_str()) {
                Some("simple")  => Complexity::Simple,
                Some("complex") => Complexity::Complex,
                _               => Complexity::Medium,
            };
            let task_type = match v.get("task_type").and_then(|t| t.as_str()) {
                Some("code")     => TaskType::Code,
                Some("research") => TaskType::Research,
                Some("creative") => TaskType::Creative,
                Some("data")     => TaskType::Data,
                _                => TaskType::Other,
            };
            let tokens = v.get("estimated_output_tokens").and_then(|t| t.as_i64()).unwrap_or(512);
            let reasoning = v.get("reasoning").and_then(|r| r.as_str()).unwrap_or("Classified by Haiku").to_string();
            Classification { complexity, task_type, estimated_output_tokens: tokens, reasoning, routed_to: String::new(), cache_hit: false }
        } else {
            Classification {
                complexity: Complexity::Medium, task_type: TaskType::Other,
                estimated_output_tokens: 512,
                reasoning: "JSON parse failed; defaulted to medium".to_string(),
                routed_to: String::new(), cache_hit: false,
            }
        }
    }

    pub async fn call_anthropic(&self, req: &AnthropicRequest) -> Result<AnthropicResponse, reqwest::Error> {
        self.client.post(ANTHROPIC_API)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(req).send().await?.json::<AnthropicResponse>().await
    }
}
