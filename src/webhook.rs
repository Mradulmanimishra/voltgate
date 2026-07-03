/// webhook.rs — spend threshold alerts (Slack-compatible webhook).
/// Fires at 80% and 100% of a caller's hourly spend limit.
/// De-duplicated per caller per threshold per hour bucket.

use std::collections::HashSet;
use std::sync::Mutex;
use once_cell::sync::Lazy;

pub const ALERT_THRESHOLDS: [f64; 2] = [0.80, 1.00];

static ALERTED: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

fn alert_key(caller_id: &str, threshold: f64, hour_bucket: i64) -> String {
    format!("{caller_id}|{threshold}|{hour_bucket}")
}

fn current_hour_bucket() -> i64 { chrono::Utc::now().timestamp() / 3600 }

pub fn check_threshold_crossed(caller_id: &str, current_spend: f64, limit: f64) -> Option<f64> {
    if limit <= 0.0 { return None; }
    let pct = current_spend / limit;
    let hour_bucket = current_hour_bucket();
    let mut alerted = ALERTED.lock().unwrap();

    if alerted.len() > 10_000 {
        alerted.retain(|k| k.rsplit('|').next().and_then(|b| b.parse::<i64>().ok()).map(|b| b >= hour_bucket - 2).unwrap_or(false));
    }

    // Check highest threshold first. If spend jumps straight past 100%
    // in a single call (e.g. one large request), we want the critical
    // 100% alert, not the milder 80% warning — even though 80% was
    // technically also crossed. Lower thresholds are marked as already
    // alerted too, so they don't separately fire on a later call.
    let mut sorted_desc = ALERT_THRESHOLDS.to_vec();
    sorted_desc.sort_by(|a, b| b.partial_cmp(a).unwrap());

    for &threshold in sorted_desc.iter() {
        if pct >= threshold {
            let key = alert_key(caller_id, threshold, hour_bucket);
            if !alerted.contains(&key) {
                // Mark this and every lower threshold as alerted, since
                // a higher-severity alert already communicates them.
                for &lower in ALERT_THRESHOLDS.iter().filter(|&&t| t <= threshold) {
                    alerted.insert(alert_key(caller_id, lower, hour_bucket));
                }
                return Some(threshold);
            }
        }
    }
    None
}

#[derive(Debug, serde::Serialize)]
pub struct SpendAlert {
    pub caller_id: String, pub threshold_pct: f64, pub current_spend: f64,
    pub limit_usd: f64, pub timestamp: String,
}

pub async fn send_spend_alert(webhook_url: &str, caller_id: &str, threshold: f64, current_spend: f64, limit_usd: f64, client: &reqwest::Client) {
    if webhook_url.is_empty() { return; }
    let severity = if threshold >= 1.0 { "🔴 LIMIT REACHED" } else { "🟡 WARNING" };
    let text = format!("{severity} — Caller `{caller_id}` has spent ${current_spend:.4} of ${limit_usd:.2} hourly limit ({:.0}%)", threshold * 100.0);
    let payload = serde_json::json!({
        "text": text,
        "alert": { "caller_id": caller_id, "threshold_pct": threshold * 100.0, "current_spend": current_spend, "limit_usd": limit_usd }
    });
    match client.post(webhook_url).json(&payload).send().await {
        Ok(resp) if resp.status().is_success() => tracing::info!(caller_id, threshold, "Spend alert sent"),
        Ok(resp) => tracing::warn!(status = %resp.status(), "Spend alert webhook returned non-2xx"),
        Err(e)   => tracing::warn!(error = %e, "Failed to send spend alert webhook"),
    }
}

pub fn maybe_alert(caller_id: String, current_spend: f64, limit_usd: f64, webhook_url: String, client: reqwest::Client) {
    if let Some(threshold) = check_threshold_crossed(&caller_id, current_spend, limit_usd) {
        tokio::spawn(async move {
            send_spend_alert(&webhook_url, &caller_id, threshold, current_spend, limit_usd, &client).await;
        });
    }
}
