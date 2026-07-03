use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

// ── Model names ────────────────────────────────────────────────────────────────

pub const HAIKU:   &str = "claude-haiku-4-5";
pub const SONNET:  &str = "claude-sonnet-4-6";
pub const OPUS:    &str = "claude-opus-4-8";
pub const FABLE:   &str = "claude-fable-5";

pub const ANTHROPIC_API: &str = "https://api.anthropic.com/v1/messages";

// ── Pricing ($ per token) ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ModelPricing {
    pub input_per_token:  f64,
    pub output_per_token: f64,
}

impl ModelPricing {
    pub fn for_model(model: &str) -> Self {
        match model {
            m if m.contains("haiku")  => Self { input_per_token: 0.25  / 1_000_000.0, output_per_token: 1.25  / 1_000_000.0 },
            m if m.contains("sonnet") => Self { input_per_token: 3.00  / 1_000_000.0, output_per_token: 15.00 / 1_000_000.0 },
            m if m.contains("opus")   => Self { input_per_token: 15.00 / 1_000_000.0, output_per_token: 75.00 / 1_000_000.0 },
            m if m.contains("fable")  => Self { input_per_token: 10.00 / 1_000_000.0, output_per_token: 50.00 / 1_000_000.0 },
            _                         => Self { input_per_token: 10.00 / 1_000_000.0, output_per_token: 50.00 / 1_000_000.0 },
        }
    }

    pub fn cost(&self, input_tokens: i64, output_tokens: i64) -> f64 {
        (input_tokens as f64 * self.input_per_token)
            + (output_tokens as f64 * self.output_per_token)
    }
}

// ── Complexity classification ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    Simple,
    Medium,
    Complex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskType {
    Code,
    Research,
    Creative,
    Data,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classification {
    pub complexity:              Complexity,
    pub task_type:               TaskType,
    pub estimated_output_tokens: i64,
    pub reasoning:               String,
    pub routed_to:               String,
    pub cache_hit:               bool,
}

impl Classification {
    pub fn route_model(&self) -> &'static str {
        match self.complexity {
            Complexity::Simple  => HAIKU,
            Complexity::Medium  => SONNET,
            Complexity::Complex => FABLE,
        }
    }
}

// ── OpenAI-compatible request / response ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIMessage {
    pub role:    String,
    pub content: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model:       Option<String>,
    pub messages:    Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens:  Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream:      Option<bool>,
    #[serde(flatten)]
    pub extra:       serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIChoice {
    pub index:         i64,
    pub message:       OpenAIMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens:     i64,
    pub completion_tokens: i64,
    pub total_tokens:      i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIResponse {
    pub id:      String,
    pub object:  String,
    pub created: i64,
    pub model:   String,
    pub choices: Vec<OpenAIChoice>,
    pub usage:   OpenAIUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_router: Option<RouterMeta>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RouterMeta {
    pub routed_to:               String,
    pub complexity:               String,
    pub task_type:                String,
    pub estimated_output_tokens:  i64,
    pub cost_usd:                 f64,
    pub cache_hit:                bool,
    pub reasoning:                String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_used:            Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_model:           Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_attempts:           Option<u32>,
}

// ── Anthropic native request / response ───────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicRequest {
    pub model:      String,
    pub messages:   Vec<OpenAIMessage>,
    pub max_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system:     Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text:       Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens:  i64,
    pub output_tokens: i64,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicResponse {
    pub id:          String,
    pub content:     Vec<AnthropicContentBlock>,
    pub usage:       AnthropicUsage,
    pub stop_reason: Option<String>,
}

// ── Database record ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ApiCallRecord {
    pub request_id:   String,
    pub routed_model: String,
    pub task_type:    String,
    pub complexity:   String,
    pub input_tokens:  i64,
    pub output_tokens: i64,
    pub cost_usd:      f64,
    pub latency_ms:    i64,
    pub cache_hit:     bool,
    pub timestamp:     DateTime<Utc>,
}

// ── ACP types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AcpRequest {
    pub task:          String,
    pub from_agent:    Option<String>,
    pub budget_usd:    Option<f64>,
    pub model_hint:    Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AcpResponse {
    pub result:          String,
    pub model_used:      String,
    pub cost_usd:        f64,
    pub budget_remaining: Option<f64>,
}

// ── Error response ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize)]
pub struct ErrorDetail {
    pub message: String,
    pub code:    String,
    #[serde(rename = "type")]
    pub kind:    String,
}

impl ErrorResponse {
    pub fn new(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                message: message.into(),
                code:    code.into(),
                kind:    "error".to_string(),
            },
        }
    }
}

// ── Guardrails config ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailsConfig {
    pub guardrails: GuardrailsInner,
    #[serde(default)]
    pub model_budgets: ModelBudgetsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailsInner {
    pub max_cost_per_request_usd: f64,
    pub max_tokens_per_request:   i64,
    pub blocked_phrases:          Vec<String>,
    pub force_model:              Option<String>,
}

impl Default for GuardrailsConfig {
    fn default() -> Self {
        Self {
            guardrails: GuardrailsInner {
                max_cost_per_request_usd: 1.00,
                max_tokens_per_request:   100_000,
                blocked_phrases:          vec![
                    "ignore previous instructions".to_string(),
                    "jailbreak".to_string(),
                ],
                force_model: None,
            },
            model_budgets: ModelBudgetsConfig::default(),
        }
    }
}

// ── Per-model daily spend budgets ─────────────────────────────────────────────
//
// Separate from the per-request cost cap above. This caps TOTAL daily
// spend per model — e.g. "never spend more than $50/day on Fable 5,
// regardless of how many individual requests that is." Requests that
// would exceed a model's daily budget are either rejected or routed
// to the fallback model, depending on `on_exceeded`.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelBudgetsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_on_exceeded")]
    pub on_exceeded: String, // "reject" | "fallback"
    #[serde(default)]
    pub daily_limits_usd: std::collections::HashMap<String, f64>,
}

fn default_on_exceeded() -> String { "fallback".to_string() }

impl Default for ModelBudgetsConfig {
    fn default() -> Self {
        let mut limits = std::collections::HashMap::new();
        limits.insert(FABLE.to_string(),  50.0);
        limits.insert(OPUS.to_string(),   30.0);
        limits.insert(SONNET.to_string(), 100.0);
        limits.insert(HAIKU.to_string(),  20.0);
        Self {
            enabled: false,
            on_exceeded: default_on_exceeded(),
            daily_limits_usd: limits,
        }
    }
}

impl ModelBudgetsConfig {
    pub fn limit_for(&self, model: &str) -> Option<f64> {
        self.daily_limits_usd.iter()
            .find(|(k, _)| model.contains(k.as_str()) || k.as_str() == model)
            .map(|(_, v)| *v)
    }

    pub fn should_fallback_on_exceed(&self) -> bool {
        self.on_exceeded == "fallback"
    }
}
