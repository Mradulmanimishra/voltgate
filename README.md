# ⚡ VoltGate

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust Version](https://img.shields.io/badge/rustc-1.78%2B-blue.svg)](https://www.rust-lang.org/)
[![Status](https://img.shields.io/badge/status-production--ready-green.svg)]()

VoltGate is an open-source, high-performance, and intelligent proxy server designed to route Anthropic requests dynamically to the cheapest model that can reliably handle the task. By combining real-time classification, advanced context engineering, failover chains, and dynamic spend controls, it allows developers to slash API costs by up to **78%** without sacrificing system reliability.

---

## 🚀 Key Features

*   **Intelligent Routing:** Automatically classifies prompt complexity using a cached Haiku classifier, routing requests to `Haiku 4.5` (simple), `Sonnet 4.6` (medium), or `Fable 5` (complex).
*   **Context Engineering:** Trims long transcripts, injects task-specific personas, and compresses conversation history when token count exceeds 8k.
*   **OpenAI Compatibility:** Serves as a drop-in replacement for both Chat Completions (`/v1/chat/completions`) and Embeddings (`/v1/embeddings`), including real-time SSE streaming.
*   **Cluster-Ready Rate Limiting:** Enforces caller RPM and spend limits using either an in-memory sliding window or a distributed Redis backend.
*   **Resiliency & Failovers:** Automatically handles transient network or provider failures (5xx, 429, 529) via exponential backoff with jitter, falling back across models if error persists.
*   **Real-time Admin UI:** Features a dark-themed live dashboard to inspect latencies, cache hit rates, model distributions, and hot-reload guardrail parameters on the fly.

---

## 🗺️ How It Works

```mermaid
graph TD
    App[Your Application] -->|HTTP Request| Auth[Bearer Auth Middleware]
    Auth -->|Check RPM & Spend| Limiter{Rate Limiter}
    Limiter -->|Passed| Classifier[Haiku Classifier & Cache]
    Classifier -->|Select Model| Guardrails{Pre-Flight Guardrails}
    Guardrails -->|Passed| Budgets{Model Daily Budgets}
    Budgets -->|Budget OK| Context[Context Engineering Engine]
    Budgets -->|Exceeded / Fallback| Reroute[Select Next Fallback Model]
    Reroute --> Context
    
    Context -->|1. Compress >8k| Pipeline[Request Pipeline]
    Context -->|2. Trim to budget| Pipeline
    Context -->|3. System Injection| Pipeline
    Context -->|4. Prompt Caching| Pipeline
    
    Pipeline -->|Check stream=true| Dispatcher{Request Dispatcher}
    Dispatcher -->|Stream| SSE[Streaming SSE Proxy]
    Dispatcher -->|Non-Stream| Proxy[Retrying HTTP Proxy]
    
    Proxy -->|529 / 429 / 5xx| Retry[Exponential Backoff Retry]
    Retry -->|Persistent Failures| Failover[Model Fallback Escalation]
    
    SSE --> DB[(SQLite Logs)]
    Proxy --> DB
    
    DB --> Dash[HTML Admin Dashboard]
    DB --> Prometheus[/metrics Endpoint]
```

### 🏛️ Module Architecture

The codebase is highly modularized inside the `src/` directory, adhering to single-responsibility design principles:

| Component | Responsibility |
| :--- | :--- |
| **`auth.rs`** | Bearer token authentication middleware for API route security. |
| **`rate_limiter.rs`** | Implements call volume (RPM) and hour-spend sliding windows (supporting both local memory or distributed Redis). |
| **`classifier.rs`** | Real-time classification engine utilizing cached Claude Haiku checks to identify complexity. |
| **`guardrails.rs`** | Pre-flight limits verifying cost limits, input token budgets, and blocked jailbreak phrases. |
| **`context_engine.rs`** | Trims message history, compresses redundant tokens, and injects customized prompts. |
| **`proxy.rs`** | Handles downstream forwarding, retry behaviors, and model fallback escalation for standard JSON completions. |
| **`streaming.rs`** | Custom Server-Sent Events (SSE) streaming engine forwarding real-time token chunks. |
| **`retry.rs`** & **`fallback.rs`** | Robust retry mechanics (exponential backoff with jitter) and deterministic model downgrades. |
| **`database.rs`** | SQLite integration logging request records, latencies, cache rates, and token counts. |
| **`dashboard.rs`** | Serving a dark-themed HTML monitoring dashboard visualizing metrics via Chart.js. |
| **`webhook.rs`** | Dispatches instant spending alerts to external endpoints (e.g. Slack) on budget thresholds. |

---

## 📦 Quick Start

### 1. Configure the Environment
Copy the example environment template and configure your keys:
```bash
cp .env.example .env
# Set ANTHROPIC_API_KEY at a minimum. Optional: VOYAGE_API_KEY & REDIS_URL.
```

### 2. Build and Run (Native)
Ensure you have the Rust compiler installed (v1.78+).
```bash
cargo build --release
./target/release/voltgate
```
*   **Dashboard:** [http://localhost:3001/dashboard](http://localhost:3001/dashboard)
*   **Prometheus Metrics:** [http://localhost:3001/metrics](http://localhost:3001/metrics)
*   **Health Status:** [http://localhost:3001/health](http://localhost:3001/health)

### 3. Build and Run (Docker)
```bash
docker-compose up --build -d
```

---

## 🔌 Integration Examples

VoltGate integrates with your current code by swapping the API endpoint.

### Python SDK (Anthropic SDK Drop-in)
```python
import anthropic

client = anthropic.Anthropic(
    api_key="your-router-key",  # Configured via ROUTER_API_KEY
    base_url="http://localhost:3001",
)

# Works for standard responses as well as streaming SSE
stream = client.messages.create(
    model="claude-sonnet-4-6", # Will be automatically routed if not forced
    max_tokens=1024,
    messages=[{"role": "user", "content": "Analyze our server performance logs."}],
    stream=True
)
for event in stream:
    if event.type == "content_block_delta":
        print(event.delta.text, end="")
```

---

## ⚡ Routing & Pricing Matrix

| Complexity | Target Model | Input $/M | Output $/M | Description |
| :--- | :--- | :--- | :--- | :--- |
| **Simple** | `claude-haiku-4-5` | $0.25 | $1.25 | Basic classification, extraction, formatting |
| **Medium** | `claude-sonnet-4-6` | $3.00 | $15.00 | Coding, general reasoning, summarization |
| **Complex** | `claude-fable-5` | $10.00 | $50.00 | High-end mathematics, agent planning, complex logic |

*Prompt caching saves up to **90%** of input costs on repeated context.*

---

## 🛡️ Reliability & Cost Controls

### Multi-Tiered Retries & Fallbacks
1.  **Exponential Backoff:** Retries transient API errors (429, 529, 500, 502, 503, 504) across 4 attempts (`200ms -> 400ms -> 800ms -> 1600ms`) with ±20% random jitter.
2.  **Failover Chain:** If a model exhausts all retries, the router automatically fails over to the next tier:
    $$\text{claude-fable-5} \longrightarrow \text{claude-sonnet-4-6} \longrightarrow \text{claude-haiku-4-5}$$
    The returned OpenAI response metadata fields (`x_router.fallback_used` and `x_router.original_model`) indicate when a failover took place.

### Configuration (`config.toml`)
Dynamically load per-request guardrails and per-model daily spend budgets:
```toml
[guardrails]
max_cost_per_request_usd = 1.50
max_tokens_per_request   = 120_000
blocked_phrases = ["ignore previous instructions", "jailbreak"]

[model_budgets]
enabled = true
on_exceeded = "fallback"  # or "reject" to yield 429 immediately

[model_budgets.daily_limits_usd]
claude-fable-5   = 50.0
claude-sonnet-4-6 = 100.0
```

---

## ⚙️ Environment Configuration

| Variable | Required | Default | Description |
| :--- | :--- | :--- | :--- |
| `ANTHROPIC_API_KEY` | **Yes** | — | Standard Anthropic credentials |
| `ROUTER_API_KEY` | No | _(open mode)_ | Bearer auth key clients must submit to access proxy |
| `VOYAGE_API_KEY` | No | _(mock fallback)_ | Voyage AI credentials to support real text embeddings |
| `REDIS_URL` | No | _(in-memory)_ | Redis URL (e.g. `redis://127.0.0.1:6379`) to activate shared rate-limiting |
| `MAX_RPM` | No | `60` | Max requests per minute allowed per caller |
| `MAX_SPEND_PER_HOUR_USD` | No | `10.0` | Max cost limit per hour allowed per caller |
| `SPEND_ALERT_WEBHOOK_URL`| No | _(disabled)_ | Slack webhook URL for spend alerts (fires at 80% / 100%) |
| `PORT` | No | `3001` | Server listening port |
| `DB_PATH` | No | `router.db` | SQLite database file location |

---

## 📡 API Reference

| Method | Path | Auth | Description |
| :--- | :--- | :--- | :--- |
| **POST** | `/v1/chat/completions` | Bearer | OpenAI-compatible chat completions proxy |
| **POST** | `/v1/embeddings` | Bearer | OpenAI-compatible text embeddings proxy (Voyage / Mock) |
| **POST** | `/acp/run` | Bearer | Agent Communication Protocol delegation endpoint |
| **GET** | `/dashboard` | Bearer | Dark-themed metrics HTML dashboard & admin panel |
| **GET** | `/api/stats` | Bearer | Cost stats, averages, and routing distributions (JSON) |
| **GET** | `/api/stats/csv` | Bearer | Download history call records as a CSV report |
| **GET** | `/api/config` | Bearer | Retrieve active in-memory guardrails configuration |
| **POST** | `/api/config` | Bearer | Update and write-back configurations to `config.toml` |
| **GET** | `/api/rate-limits` | Bearer | Fetch client rate limit and hourly spend snapshots |
| **GET** | `/metrics` | Open | Prometheus scraper statistics endpoint |
| **GET** | `/health` | Open | System heartbeat endpoint (`{"status": "ok"}`) |

---

## 🧪 Development & Testing

VoltGate includes a comprehensive test suite of 108 unit and integration tests validating the routing logic, rate limiter, context engineering, and guardrails.

### Running Tests

**On Linux / macOS:**
```bash
cargo test
```

**On Windows (PowerShell, using the bundled MinGW compatibility libraries):**
```powershell
$env:RUSTFLAGS = "-L $PWD\gcc_compat"
cargo test
```

---

## 📄 License
This project is licensed under the MIT License — see the [LICENSE](LICENSE) file for details.
