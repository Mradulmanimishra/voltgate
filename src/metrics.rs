use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::collections::HashMap;
use std::sync::Mutex;
use once_cell::sync::Lazy;

pub static METRICS: Lazy<Metrics> = Lazy::new(Metrics::new);

#[derive(Default)]
pub(crate) struct Histogram { values: Mutex<Vec<f64>> }
impl Histogram {
    fn record(&self, v: f64) { self.values.lock().unwrap().push(v); }
    fn quantile(&self, q: f64) -> f64 {
        let mut vals = self.values.lock().unwrap().clone();
        if vals.is_empty() { return 0.0; }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = ((vals.len() as f64 * q) as usize).min(vals.len() - 1);
        vals[idx]
    }
    fn count(&self) -> usize { self.values.lock().unwrap().len() }
    fn sum(&self) -> f64 { self.values.lock().unwrap().iter().sum() }
}

pub struct Metrics {
    pub requests_total:       Mutex<HashMap<String, u64>>,
    pub cost_usd_total:       Mutex<HashMap<String, f64>>,
    pub cache_hits:           AtomicU64,
    pub cache_misses:         AtomicU64,
    pub compression_applied:  AtomicU64,
    pub guardrail_violations: Mutex<HashMap<String, u64>>,
    pub active_requests:      AtomicI64,
    pub(crate) latency_ms:     Mutex<HashMap<String, Histogram>>,
    pub(crate) context_tokens: Mutex<HashMap<String, Histogram>>,
    pub retries_total:        AtomicU64,
    pub fallbacks_total:      AtomicU64,
}

impl Metrics {
    fn new() -> Self {
        Self {
            requests_total: Mutex::new(HashMap::new()), cost_usd_total: Mutex::new(HashMap::new()),
            cache_hits: AtomicU64::new(0), cache_misses: AtomicU64::new(0),
            compression_applied: AtomicU64::new(0), guardrail_violations: Mutex::new(HashMap::new()),
            active_requests: AtomicI64::new(0), latency_ms: Mutex::new(HashMap::new()),
            context_tokens: Mutex::new(HashMap::new()),
            retries_total: AtomicU64::new(0), fallbacks_total: AtomicU64::new(0),
        }
    }

    pub fn request_started(&self) { self.active_requests.fetch_add(1, Ordering::Relaxed); }

    pub fn request_finished(&self, model: &str, complexity: &str, task_type: &str, status: &str,
        cost_usd: f64, latency_ms: f64, ctx_tokens: usize, cache_hit: bool, compressed: bool) {
        self.active_requests.fetch_sub(1, Ordering::Relaxed);
        let req_key = format!("{model}|{complexity}|{task_type}|{status}");
        *self.requests_total.lock().unwrap().entry(req_key).or_insert(0) += 1;
        *self.cost_usd_total.lock().unwrap().entry(model.to_string()).or_insert(0.0) += cost_usd;
        if cache_hit { self.cache_hits.fetch_add(1, Ordering::Relaxed); } else { self.cache_misses.fetch_add(1, Ordering::Relaxed); }
        if compressed { self.compression_applied.fetch_add(1, Ordering::Relaxed); }
        self.latency_ms.lock().unwrap().entry(model.to_string()).or_insert_with(Histogram::default).record(latency_ms);
        self.context_tokens.lock().unwrap().entry(model.to_string()).or_insert_with(Histogram::default).record(ctx_tokens as f64);
    }

    pub fn guardrail_violated(&self, reason: &str) {
        *self.guardrail_violations.lock().unwrap().entry(reason.to_string()).or_insert(0) += 1;
    }

    pub fn retry_occurred(&self) { self.retries_total.fetch_add(1, Ordering::Relaxed); }
    pub fn fallback_occurred(&self) { self.fallbacks_total.fetch_add(1, Ordering::Relaxed); }

    pub fn render(&self) -> String {
        let mut out = String::with_capacity(4096);
        let active = self.active_requests.load(Ordering::Relaxed);
        out.push_str("# HELP voltgate_active_requests Number of requests currently being processed\n# TYPE voltgate_active_requests gauge\n");
        out.push_str(&format!("voltgate_active_requests {active}\n\n"));

        out.push_str("# HELP voltgate_requests_total Total requests processed\n# TYPE voltgate_requests_total counter\n");
        for (key, count) in self.requests_total.lock().unwrap().iter() {
            let parts: Vec<&str> = key.split('|').collect();
            if parts.len() == 4 {
                out.push_str(&format!("voltgate_requests_total{{model=\"{}\",complexity=\"{}\",task_type=\"{}\",status=\"{}\"}} {count}\n", parts[0], parts[1], parts[2], parts[3]));
            }
        }
        out.push('\n');

        out.push_str("# HELP voltgate_cost_usd_total Total cost in USD\n# TYPE voltgate_cost_usd_total counter\n");
        for (model, cost) in self.cost_usd_total.lock().unwrap().iter() {
            out.push_str(&format!("voltgate_cost_usd_total{{model=\"{model}\"}} {cost:.8}\n"));
        }
        out.push('\n');

        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        out.push_str(&format!("# HELP voltgate_cache_hits_total Classifier cache hits\n# TYPE voltgate_cache_hits_total counter\nvoltgate_cache_hits_total {hits}\n"));
        out.push_str(&format!("# HELP voltgate_cache_misses_total Classifier cache misses\n# TYPE voltgate_cache_misses_total counter\nvoltgate_cache_misses_total {misses}\n\n"));

        let comp = self.compression_applied.load(Ordering::Relaxed);
        out.push_str(&format!("# HELP voltgate_compression_applied_total Requests where context was compressed\n# TYPE voltgate_compression_applied_total counter\nvoltgate_compression_applied_total {comp}\n\n"));

        let retries = self.retries_total.load(Ordering::Relaxed);
        let fallbacks = self.fallbacks_total.load(Ordering::Relaxed);
        out.push_str(&format!("# HELP voltgate_retries_total Requests that were retried\n# TYPE voltgate_retries_total counter\nvoltgate_retries_total {retries}\n"));
        out.push_str(&format!("# HELP voltgate_fallbacks_total Requests that fell back to a different model\n# TYPE voltgate_fallbacks_total counter\nvoltgate_fallbacks_total {fallbacks}\n\n"));

        out.push_str("# HELP voltgate_guardrail_violations_total Guardrail violations by reason\n# TYPE voltgate_guardrail_violations_total counter\n");
        for (reason, count) in self.guardrail_violations.lock().unwrap().iter() {
            out.push_str(&format!("voltgate_guardrail_violations_total{{reason=\"{reason}\"}} {count}\n"));
        }
        out.push('\n');

        out.push_str("# HELP voltgate_latency_ms Request latency in milliseconds\n# TYPE voltgate_latency_ms summary\n");
        for (model, hist) in self.latency_ms.lock().unwrap().iter() {
            for (q_name, q_val) in [("0.5", 0.5_f64), ("0.95", 0.95), ("0.99", 0.99)] {
                out.push_str(&format!("voltgate_latency_ms{{model=\"{model}\",quantile=\"{q_name}\"}} {:.1}\n", hist.quantile(q_val)));
            }
            out.push_str(&format!("voltgate_latency_ms_sum{{model=\"{model}\"}} {:.1}\n", hist.sum()));
            out.push_str(&format!("voltgate_latency_ms_count{{model=\"{model}\"}} {}\n", hist.count()));
        }
        out.push('\n');

        out.push_str("# HELP voltgate_context_tokens Context tokens sent per request\n# TYPE voltgate_context_tokens summary\n");
        for (model, hist) in self.context_tokens.lock().unwrap().iter() {
            for (q_name, q_val) in [("0.5", 0.5_f64), ("0.95", 0.95)] {
                out.push_str(&format!("voltgate_context_tokens{{model=\"{model}\",quantile=\"{q_name}\"}} {:.0}\n", hist.quantile(q_val)));
            }
        }
        out
    }
}
