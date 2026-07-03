/// main.rs — VoltGate (fully wired: CE, retry, fallback, streaming,
/// per-model budgets, webhook alerts)

mod models;
mod database;
mod classifier;
mod guardrails;
mod proxy;
mod dashboard;
mod acp;
mod context_engine;
mod auth;
mod rate_limiter;
mod metrics;
mod retry;
mod fallback;
mod webhook;
mod streaming;
mod state;
mod embeddings;

use std::sync::Arc;
use axum::{
    extract::State, http::{HeaderMap, StatusCode}, middleware,
    response::{Html, IntoResponse, Response}, routing::{get, post}, Json, Router,
};
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use models::{ErrorResponse, GuardrailsConfig, OpenAIRequest};
use classifier::Classifier;
use proxy::Proxy;
use rate_limiter::RateLimiter;
use metrics::METRICS;
use state::AppState;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "voltgate=info,tower_http=info".into()))
        .init();

    let api_key      = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");
    let db_path       = std::env::var("DB_PATH").unwrap_or_else(|_| "router.db".into());
    let port          = std::env::var("PORT").unwrap_or_else(|_| "3001".into());
    let config_path   = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".into());
    let max_rpm       = std::env::var("MAX_RPM").unwrap_or_else(|_| "60".into()).parse::<usize>().unwrap_or(60);
    let max_spend_hr  = std::env::var("MAX_SPEND_PER_HOUR_USD").unwrap_or_else(|_| "10.0".into()).parse::<f64>().unwrap_or(10.0);
    let webhook_url   = std::env::var("SPEND_ALERT_WEBHOOK_URL").unwrap_or_default();

    let guardrails: GuardrailsConfig = if std::path::Path::new(&config_path).exists() {
        let raw = std::fs::read_to_string(&config_path).expect("Failed to read config.toml");
        toml::from_str(&raw).expect("Invalid config.toml")
    } else {
        tracing::warn!("config.toml not found — using defaults");
        GuardrailsConfig::default()
    };

    let db           = database::init_db(&db_path).expect("Failed to open SQLite");
    let classifier   = Classifier::new(api_key.clone());
    let proxy        = Proxy::new(api_key.clone());
    let rate_limiter = Arc::new(RateLimiter::new());
    let http_client  = reqwest::Client::new();

    tracing::info!("Database ready at {db_path}");
    tracing::info!("Rate limits: {max_rpm} rpm, ${max_spend_hr:.2}/hr");
    if !webhook_url.is_empty() { tracing::info!("Spend alerts enabled"); }

    let state = AppState {
        db, classifier, proxy, guardrails: Arc::new(tokio::sync::RwLock::new(guardrails)), rate_limiter,
        api_key, max_rpm, max_spend_hr, webhook_url, http_client,
    };

    let protected = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/embeddings",       post(embeddings_handler))
        .route("/acp/run",             post(acp::handle_acp))
        .route("/dashboard",           get(dashboard_handler))
        .route("/api/stats",           get(api_stats))
        .route("/api/stats/csv",       get(api_stats_csv))
        .route("/api/config",          get(get_config).post(post_config))
        .route("/api/rate-limits",     get(api_rate_limits))
        .route("/api/calls",           get(api_calls))
        .layer(middleware::from_fn(auth::require_auth));

    let public = Router::new()
        .route("/health",  get(health))
        .route("/metrics", get(prometheus_metrics));

    let app = Router::new()
        .merge(protected).merge(public)
        .with_state(state)
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any));

    let addr     = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("VoltGate  ➜  http://{addr}");
    tracing::info!("Dashboard   ➜  http://{addr}/dashboard");
    tracing::info!("Metrics     ➜  http://{addr}/metrics");
    axum::serve(listener, app).await.unwrap();
}

async fn chat_completions(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<OpenAIRequest>) -> Response {
    let caller_id = headers.get("x-caller-id").and_then(|v| v.to_str().ok()).unwrap_or("anonymous").to_string();

    if let Err(e) = state.rate_limiter.check_request(&caller_id, state.max_rpm).await {
        METRICS.guardrail_violated("rate_limit_rpm");
        return (StatusCode::TOO_MANY_REQUESTS, Json(ErrorResponse::new(e.to_string(), "rate_limit"))).into_response();
    }

    let mut classification = state.classifier.classify(&req.messages).await;
    let guardrails_lock = state.guardrails.read().await;
    let routed_model = guardrails_lock.guardrails.force_model.clone()
        .or_else(|| req.model.clone())
        .unwrap_or_else(|| classification.route_model().to_string());
    classification.routed_to = routed_model.clone();

    if let Err(violation) = guardrails::check(&*guardrails_lock, &req, &routed_model, classification.estimated_output_tokens) {
        let reason = violation.to_string();
        METRICS.guardrail_violated(&reason);
        return (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(reason, "guardrail_violation"))).into_response();
    }

    // Per-model daily budget check
    if let Err(violation) = guardrails::check_model_budget(&*guardrails_lock, &state.db, &routed_model) {
        if guardrails_lock.model_budgets.should_fallback_on_exceed() {
            tracing::warn!("{violation} — falling back to next model in chain");
            if let Some(next) = fallback::next_fallback(&routed_model) {
                classification.routed_to = next.to_string();
            }
        } else {
            METRICS.guardrail_violated("model_daily_budget");
            return (StatusCode::TOO_MANY_REQUESTS, Json(ErrorResponse::new(violation.to_string(), "model_budget_exceeded"))).into_response();
        }
    }
    let routed_model = classification.routed_to.clone();
    drop(guardrails_lock);

    tracing::info!(complexity = ?classification.complexity, task_type = ?classification.task_type, routed_to = %routed_model, cache_hit = %classification.cache_hit, "Routing request");

    // Streaming path
    if req.stream == Some(true) {
        let (user_system, user_messages) = split_system_for_stream(&req);
        let (ce_body, _report) = context_engine::prepare_request(
            &user_messages, req.max_tokens, req.temperature, &classification,
            &routed_model, &state.api_key, &state.http_client, user_system,
        ).await;

        let stream = streaming::stream_chat_completion(
            ce_body, state.api_key.clone(), routed_model, classification,
            state.db.clone(), state.http_client.clone(),
        );
        return stream.into_response();
    }

    // Non-streaming path
    match state.proxy.forward(&req, &routed_model, &classification, &state.db).await {
        Ok((resp, ce_report, fallback_outcome)) => {
            let cost = resp.x_router.as_ref().map(|r| r.cost_usd).unwrap_or(0.0);

            let total_spend = match state.rate_limiter.record_spend(&caller_id, cost, state.max_spend_hr).await {
                Ok(total) => total,
                Err(e) => { tracing::warn!("Spend limit exceeded for {caller_id}: {e}"); state.max_spend_hr }
            };

            if !state.webhook_url.is_empty() {
                webhook::maybe_alert(caller_id.clone(), total_spend, state.max_spend_hr, state.webhook_url.clone(), state.http_client.clone());
            }

            tracing::info!(
                tokens_in = ce_report.final_tokens, compressed = ce_report.compression_applied,
                trimmed = ce_report.trim_applied, budget_pct = ce_report.budget_used_pct,
                cost_usd = cost, fallback_used = fallback_outcome.fallback_used,
                "Request complete"
            );

            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(msg) => {
            tracing::error!("Proxy error: {msg}");
            (StatusCode::BAD_GATEWAY, Json(ErrorResponse::new(msg, "proxy_error"))).into_response()
        }
    }
}

fn split_system_for_stream(req: &OpenAIRequest) -> (Option<String>, Vec<models::OpenAIMessage>) {
    let mut system: Option<String> = None;
    let messages = req.messages.iter().filter_map(|m| {
        if m.role == "system" {
            system = Some(match &m.content { serde_json::Value::String(s) => s.clone(), other => other.to_string() });
            None
        } else { Some(m.clone()) }
    }).collect();
    (system, messages)
}

async fn dashboard_handler(State(state): State<AppState>) -> Html<String> { Html(dashboard::render(&state.db)) }

async fn api_stats(State(state): State<AppState>) -> Json<serde_json::Value> {
    let db = &state.db;
    Json(serde_json::json!({
        "cost_today": database::cost_today(db), "cost_by_model": database::cost_by_model(db),
        "daily_cost_14d": database::daily_cost(db, 14), "avg_latency": database::avg_latency(db),
        "routing_distribution": database::routing_distribution(db), "cache_hit_rate_pct": database::cache_hit_rate(db),
    }))
}

async fn api_calls(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "calls": database::recent_calls(&state.db, 50) }))
}

async fn api_rate_limits(State(state): State<AppState>) -> Json<serde_json::Value> {
    let snapshot = state.rate_limiter.snapshot().await;
    Json(serde_json::json!({
        "config": { "max_rpm": state.max_rpm, "max_spend_per_hour_usd": state.max_spend_hr },
        "callers": snapshot.iter().map(|(id, reqs, spend)| serde_json::json!({ "caller_id": id, "requests_1min": reqs, "spend_1hr_usd": spend })).collect::<Vec<_>>()
    }))
}

async fn embeddings_handler(
    State(state): State<AppState>,
    Json(req): Json<embeddings::EmbeddingsRequest>,
) -> Response {
    match embeddings::handle_embeddings(req, &state.http_client).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(err) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": { "message": err, "type": "embeddings_error" } }))).into_response(),
    }
}

async fn api_stats_csv(State(state): State<AppState>) -> Response {
    match database::export_calls_to_csv(&state.db) {
        Ok(csv_data) => {
            Response::builder()
                .status(StatusCode::OK)
                .header(axum::http::header::CONTENT_TYPE, "text/csv")
                .header(axum::http::header::CONTENT_DISPOSITION, "attachment; filename=\"cost_report.csv\"")
                .body(axum::body::Body::from(csv_data))
                .unwrap()
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response()
        }
    }
}

async fn get_config(State(state): State<AppState>) -> Json<GuardrailsConfig> {
    let guardrails = state.guardrails.read().await;
    Json((*guardrails).clone())
}

async fn post_config(State(state): State<AppState>, Json(new_config): Json<GuardrailsConfig>) -> Response {
    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".into());
    let toml_str = match toml::to_string_pretty(&new_config) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": format!("TOML serialization failed: {}", e) }))).into_response(),
    };
    if let Err(e) = std::fs::write(&config_path, toml_str) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("Failed to write config.toml: {}", e) }))).into_response();
    }
    let mut guardrails = state.guardrails.write().await;
    *guardrails = new_config;
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok", "message": "Configuration updated and persisted successfully" }))).into_response()
}

async fn prometheus_metrics() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")], METRICS.render())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "voltgate", "version": env!("CARGO_PKG_VERSION") }))
}
