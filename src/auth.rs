use axum::{body::Body, http::{Request, StatusCode}, middleware::Next, response::{IntoResponse, Response}, Json};
use crate::models::ErrorResponse;

const BEARER_PREFIX: &str = "Bearer ";

pub async fn require_auth(request: Request<Body>, next: Next) -> Response {
    let router_key = std::env::var("ROUTER_API_KEY").unwrap_or_default();
    if router_key.is_empty() {
        tracing::debug!("Auth: open mode (ROUTER_API_KEY not set)");
        return next.run(request).await;
    }
    let auth_header = request.headers().get("authorization").and_then(|v| v.to_str().ok());
    match auth_header {
        Some(value) if value.starts_with(BEARER_PREFIX) => {
            let token = &value[BEARER_PREFIX.len()..];
            if constant_time_eq(token, &router_key) { next.run(request).await }
            else { auth_error("Invalid API key") }
        }
        Some(_) => auth_error("Authorization header must use Bearer scheme"),
        None    => auth_error("Authorization header is missing"),
    }
}

fn auth_error(msg: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(ErrorResponse::new(msg, "auth_error"))).into_response()
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() { return false; }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
