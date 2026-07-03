use std::{collections::HashMap, sync::{Arc, Mutex}, time::{Duration, Instant}};
use redis::AsyncCommands;

const REQUEST_WINDOW: Duration = Duration::from_secs(60);
const SPEND_WINDOW:   Duration = Duration::from_secs(3600);

#[derive(Clone)]
pub enum RateLimiterBackend {
    InMemory(Arc<Mutex<Inner>>),
    Redis(redis::Client),
}

#[derive(Clone)]
pub struct RateLimiter {
    pub backend: RateLimiterBackend,
}

#[derive(Default)]
pub struct Inner {
    pub requests: HashMap<String, WindowCounter>,
    pub spend:    HashMap<String, SpendCounter>,
}

pub struct WindowCounter {
    pub timestamps: std::collections::VecDeque<Instant>,
}

impl WindowCounter {
    pub fn new() -> Self {
        Self { timestamps: Default::default() }
    }
    pub fn count_in_window(&mut self, window: Duration) -> usize {
        let cutoff = Instant::now() - window;
        self.timestamps.retain(|&t| t > cutoff);
        self.timestamps.len()
    }
    pub fn record(&mut self) {
        self.timestamps.push_back(Instant::now());
    }
}

pub struct SpendCounter {
    pub total_usd:    f64,
    pub window_start: Instant,
}

impl SpendCounter {
    pub fn new() -> Self {
        Self { total_usd: 0.0, window_start: Instant::now() }
    }
    pub fn add(&mut self, amount: f64, window: Duration) -> f64 {
        if self.window_start.elapsed() > window {
            self.total_usd = 0.0;
            self.window_start = Instant::now();
        }
        self.total_usd += amount;
        self.total_usd
    }
}

#[derive(Debug)]
pub enum RateLimitError {
    TooManyRequests { limit: usize, window_secs: u64 },
    SpendLimitExceeded { spent: f64, limit: f64, window_secs: u64 },
    RedisError(String),
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyRequests { limit, window_secs } => {
                write!(f, "Rate limit: max {limit} requests per {window_secs}s")
            }
            Self::SpendLimitExceeded { spent, limit, window_secs } => {
                write!(f, "Spend limit: ${spent:.4} of ${limit:.2} used in last {window_secs}s")
            }
            Self::RedisError(e) => {
                write!(f, "Redis rate limiter error: {e}")
            }
        }
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        let redis_url = std::env::var("REDIS_URL").unwrap_or_default();
        let backend = if !redis_url.is_empty() {
            match redis::Client::open(redis_url) {
                Ok(client) => {
                    tracing::info!("Rate limiter: using Redis backend");
                    RateLimiterBackend::Redis(client)
                }
                Err(e) => {
                    tracing::error!("Failed to connect to Redis: {e}. Falling back to in-memory.");
                    RateLimiterBackend::InMemory(Arc::new(Mutex::new(Inner::default())))
                }
            }
        } else {
            tracing::info!("Rate limiter: using in-memory backend");
            RateLimiterBackend::InMemory(Arc::new(Mutex::new(Inner::default())))
        };
        Self { backend }
    }

    pub async fn check_request(&self, caller_id: &str, max_rpm: usize) -> Result<(), RateLimitError> {
        match &self.backend {
            RateLimiterBackend::InMemory(mutex) => {
                let mut inner = mutex.lock().unwrap();
                let counter = inner.requests.entry(caller_id.to_string()).or_insert_with(WindowCounter::new);
                let count = counter.count_in_window(REQUEST_WINDOW);
                if count >= max_rpm {
                    return Err(RateLimitError::TooManyRequests { limit: max_rpm, window_secs: REQUEST_WINDOW.as_secs() });
                }
                counter.record();
                Ok(())
            }
            RateLimiterBackend::Redis(client) => {
                let mut conn = client.get_async_connection().await
                    .map_err(|e| RateLimitError::RedisError(e.to_string()))?;
                
                let now = chrono::Utc::now().timestamp_millis();
                let clear_before = now - 60_000;
                let key = format!("rate_limit:rpm:{}", caller_id);
                
                let (_, count): (u64, usize) = redis::pipe()
                    .atomic()
                    .cmd("ZREMRANGEBYSCORE").arg(&key).arg(0).arg(clear_before)
                    .cmd("ZCARD").arg(&key)
                    .query_async(&mut conn).await
                    .map_err(|e| RateLimitError::RedisError(e.to_string()))?;
                
                if count >= max_rpm {
                    return Err(RateLimitError::TooManyRequests { limit: max_rpm, window_secs: 60 });
                }
                
                let uuid = uuid::Uuid::new_v4().to_string();
                let _: () = redis::pipe()
                    .atomic()
                    .cmd("ZADD").arg(&key).arg(now).arg(&uuid)
                    .cmd("EXPIRE").arg(&key).arg(60)
                    .query_async(&mut conn).await
                    .map_err(|e| RateLimitError::RedisError(e.to_string()))?;
                
                Ok(())
            }
        }
    }

    pub async fn record_spend(&self, caller_id: &str, cost_usd: f64, max_spend_per_hour: f64) -> Result<f64, RateLimitError> {
        match &self.backend {
            RateLimiterBackend::InMemory(mutex) => {
                let mut inner = mutex.lock().unwrap();
                let counter = inner.spend.entry(caller_id.to_string()).or_insert_with(SpendCounter::new);
                let total = counter.add(cost_usd, SPEND_WINDOW);
                if total > max_spend_per_hour {
                    return Err(RateLimitError::SpendLimitExceeded { spent: total, limit: max_spend_per_hour, window_secs: SPEND_WINDOW.as_secs() });
                }
                Ok(total)
            }
            RateLimiterBackend::Redis(client) => {
                let mut conn = client.get_async_connection().await
                    .map_err(|e| RateLimitError::RedisError(e.to_string()))?;
                let key = format!("rate_limit:spend:hourly:{}", caller_id);
                
                let script = redis::Script::new(r#"
                    local current = redis.call('get', KEYS[1])
                    if not current then
                        redis.call('set', KEYS[1], ARGV[1])
                        redis.call('expire', KEYS[1], ARGV[2])
                        return tonumber(ARGV[1])
                    else
                        return redis.call('incrbyfloat', KEYS[1], ARGV[1])
                    end
                "#);
                
                let total: f64 = script.key(&key).arg(cost_usd).arg(3600)
                    .invoke_async(&mut conn).await
                    .map_err(|e| RateLimitError::RedisError(e.to_string()))?;
                
                if total > max_spend_per_hour {
                    return Err(RateLimitError::SpendLimitExceeded { spent: total, limit: max_spend_per_hour, window_secs: 3600 });
                }
                Ok(total)
            }
        }
    }

    pub async fn snapshot(&self) -> Vec<(String, usize, f64)> {
        match &self.backend {
            RateLimiterBackend::InMemory(mutex) => {
                let mut inner = mutex.lock().unwrap();
                let counts: Vec<(String, usize)> = inner.requests.iter_mut()
                    .map(|(id, rc)| (id.clone(), rc.count_in_window(REQUEST_WINDOW)))
                    .collect();
                counts.into_iter()
                    .map(|(id, reqs)| {
                        let spend = inner.spend.get(&id).map(|s| s.total_usd).unwrap_or(0.0);
                        (id, reqs, spend)
                    })
                    .collect()
            }
            RateLimiterBackend::Redis(client) => {
                let mut conn = match client.get_async_connection().await {
                    Ok(c) => c,
                    Err(_) => return vec![],
                };
                
                let rpm_pattern = "rate_limit:rpm:*";
                let spend_pattern = "rate_limit:spend:hourly:*";
                let mut caller_ids = std::collections::HashSet::new();
                
                let mut cursor: u64 = 0;
                loop {
                    let (new_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                        .arg(cursor)
                        .arg("MATCH").arg(rpm_pattern)
                        .query_async(&mut conn).await.unwrap_or((0, vec![]));
                    for key in keys {
                        if let Some(id) = key.strip_prefix("rate_limit:rpm:") {
                            caller_ids.insert(id.to_string());
                        }
                    }
                    cursor = new_cursor;
                    if cursor == 0 { break; }
                }
                
                let mut cursor: u64 = 0;
                loop {
                    let (new_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                        .arg(cursor)
                        .arg("MATCH").arg(spend_pattern)
                        .query_async(&mut conn).await.unwrap_or((0, vec![]));
                    for key in keys {
                        if let Some(id) = key.strip_prefix("rate_limit:spend:hourly:") {
                            caller_ids.insert(id.to_string());
                        }
                    }
                    cursor = new_cursor;
                    if cursor == 0 { break; }
                }
                
                let mut result = vec![];
                for id in caller_ids {
                    let rpm_key = format!("rate_limit:rpm:{}", id);
                    let spend_key = format!("rate_limit:spend:hourly:{}", id);
                    
                    let reqs: usize = conn.zcard(&rpm_key).await.unwrap_or(0);
                    let spend: f64 = conn.get(&spend_key).await.unwrap_or(0.0);
                    
                    result.push((id, reqs, spend));
                }
                result
            }
        }
    }
}
