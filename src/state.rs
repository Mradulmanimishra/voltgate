/// state.rs — shared application state.
/// Lives in the library (not main.rs) so library modules like acp.rs
/// can reference it too.

use std::sync::Arc;
use crate::models::GuardrailsConfig;
use crate::database::Db;
use crate::classifier::Classifier;
use crate::proxy::Proxy;
use crate::rate_limiter::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub db:           Db,
    pub classifier:   Classifier,
    pub proxy:        Proxy,
    pub guardrails:   Arc<tokio::sync::RwLock<GuardrailsConfig>>,
    pub rate_limiter: Arc<RateLimiter>,
    pub api_key:      String,
    pub max_rpm:      usize,
    pub max_spend_hr: f64,
    pub webhook_url:  String,
    pub http_client:  reqwest::Client,
}
