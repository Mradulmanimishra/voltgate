# VoltGate — Claude Code Instructions

## Project overview
Rust (axum) reverse proxy routing AI tasks to the cheapest reliable
Anthropic model. Classifies with Haiku, applies context engineering,
retries transient failures, falls back across models, streams via SSE,
and enforces cost limits at three levels (per-request, per-model/day,
per-caller/hour).

## Architecture

```
src/
├── main.rs           Entry point. Wires ALL modules.
├── models.rs         ALL types. Pricing. Per-model budgets.
├── classifier.rs     Haiku classifier, SHA-256 cache (1hr TTL).
├── context_engine.rs Compress / trim / cache / inject system prompt.
├── proxy.rs           CE → retry → fallback chain → Anthropic (non-stream).
├── streaming.rs        SSE path — same CE, different transport.
├── retry.rs             Exponential backoff (200ms→1.6s, 4 attempts).
├── fallback.rs           fable→sonnet→haiku chain on persistent failure.
├── webhook.rs             Spend alerts at 80%/100% of hourly limit.
├── database.rs           SQLite — cost log, dashboard queries, per-model spend.
├── guardrails.rs          Pre-flight: cost/tokens/phrases + daily model budgets.
├── auth.rs                Bearer token middleware.
├── rate_limiter.rs        Per-caller RPM + hourly spend guard.
├── metrics.rs              Prometheus counters/histograms incl. retries/fallbacks.
├── dashboard.rs             HTML dashboard (Chart.js, dark theme).
└── acp.rs                   Agent Communication Protocol endpoint.
```

## Request pipeline (respect this order when modifying)

```
HTTP request
  → auth (reject if bad token)
  → rate_limiter::check_request (reject if over RPM)
  → classifier::classify (Haiku, cached)
  → guardrails::check (cost/tokens/phrases)
  → guardrails::check_model_budget (daily per-model cap)
       exceeded + on_exceeded="fallback" → reroute via fallback::next_fallback
       exceeded + on_exceeded="reject"   → 429
  → IF stream=true:
      context_engine::prepare_request → streaming::stream_chat_completion
    ELSE:
      proxy::forward
        → context_engine::prepare_request
        → for each model in fallback::fallback_chain(routed_model):
            retry::with_retry( POST to Anthropic )
            success → break, log to SQLite, record metrics
            exhausted retries → try next model in chain
  → rate_limiter::record_spend
  → webhook::maybe_alert (if spend crossed 80%/100%)
  → return response (JSON or SSE stream)
```

## Key invariants — do not break these

- **Never call Anthropic directly from a handler.** Always go through
  `proxy::forward` (non-streaming) or `streaming::stream_chat_completion`.
- **Never query SQLite directly from a handler.** Use `database.rs` functions.
- **context_engine::prepare_request must run before every Anthropic call**,
  streaming or not — it's the entire point of this project.
- **retry.rs only retries 529/500/502/503/504/429.** Never add 4xx codes here.
- **fallback.rs chains must terminate** — `fallback_chain()` caps at 4 hops
  and dedupes to prevent cycles. Don't remove that guard.
- **webhook.rs alerts are deduplicated per (caller, threshold, hour-bucket).**
  Don't call `send_spend_alert` directly from handlers — use `maybe_alert`.

## How to add a new route
1. Handler function in `main.rs` (or a new module)
2. Register in `protected` or `public` Router in `main()`
3. DB access → add a query fn in `database.rs`
4. Add a test in `tests/`

## How to add a new model
1. Constant in `models.rs`
2. Pricing in `ModelPricing::for_model()`
3. Fallback target in `fallback::next_fallback()`
4. Default daily budget in `ModelBudgetsConfig::default()`
5. Update README pricing table

## How to add a new guardrail
Add a variant to `GuardrailViolation` in `guardrails.rs`, implement the
check, call `METRICS.guardrail_violated("reason")`.

## Running locally
```bash
cp .env.example .env   # add ANTHROPIC_API_KEY
cargo run
# http://localhost:3001/dashboard
```

## Running tests
```bash
cargo test                    # all tests
cargo test test_retry -- --nocapture
```

## Build for production
```bash
cargo build --release
```

## Environment variables
See `.env.example`. Key additions beyond the basics:
- `SPEND_ALERT_WEBHOOK_URL` — Slack webhook for 80%/100% spend alerts
- Per-model daily budgets live in `config.toml`, not env vars

## DO NOT
- Do not put business logic in `main.rs` — handlers call modules
- Do not skip `context_engine` for streaming OR non-streaming paths
- Do not hardcode model name strings — use constants from `models.rs`
- Do not commit `.env` or `router.db`

## Remaining roadmap (pick one to start)
- [ ] Redis backend for `rate_limiter` (multi-instance deployments)
- [ ] `/v1/embeddings` endpoint
- [ ] Admin UI for editing `config.toml` guardrails without restart
- [ ] CSV export of cost reports from `/api/stats`
