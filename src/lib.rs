// lib.rs — re-exports all modules so integration tests and binaries
// can both use `voltgate::module::item`.

pub mod models;
pub mod database;
pub mod classifier;
pub mod context_engine;
pub mod guardrails;
pub mod proxy;
pub mod dashboard;
pub mod acp;
pub mod auth;
pub mod rate_limiter;
pub mod metrics;
pub mod retry;
pub mod fallback;
pub mod webhook;
pub mod streaming;
pub mod state;
pub mod embeddings;
pub mod docs;
