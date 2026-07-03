use rusqlite::{Connection, Result, params};
use std::sync::{Arc, Mutex};
use crate::models::ApiCallRecord;

pub type Db = Arc<Mutex<Connection>>;

pub fn init_db(path: &str) -> Result<Db> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS api_calls (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id    TEXT    NOT NULL,
            routed_model  TEXT    NOT NULL,
            task_type     TEXT    NOT NULL,
            complexity    TEXT    NOT NULL,
            input_tokens  INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cost_usd      REAL    NOT NULL DEFAULT 0,
            latency_ms    INTEGER NOT NULL DEFAULT 0,
            cache_hit     INTEGER NOT NULL DEFAULT 0,
            timestamp     TEXT    NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_api_timestamp ON api_calls(timestamp);
        CREATE INDEX IF NOT EXISTS idx_api_model ON api_calls(routed_model);
    ")?;
    Ok(Arc::new(Mutex::new(conn)))
}

pub fn insert_call(db: &Db, rec: &ApiCallRecord) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO api_calls
             (request_id, routed_model, task_type, complexity,
              input_tokens, output_tokens, cost_usd, latency_ms,
              cache_hit, timestamp)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            rec.request_id, rec.routed_model, rec.task_type, rec.complexity,
            rec.input_tokens, rec.output_tokens, rec.cost_usd, rec.latency_ms,
            rec.cache_hit as i64, rec.timestamp.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn cost_today(db: &Db) -> f64 {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT COALESCE(SUM(cost_usd),0) FROM api_calls WHERE date(timestamp)=date('now')",
        [], |r| r.get::<_, f64>(0),
    ).unwrap_or(0.0)
}

/// Total spend on a specific model today. Used by per-model daily budgets.
pub fn model_spend_today(db: &Db, model: &str) -> f64 {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT COALESCE(SUM(cost_usd),0) FROM api_calls
          WHERE date(timestamp)=date('now') AND routed_model LIKE ?1",
        params![format!("%{model}%")],
        |r| r.get::<_, f64>(0),
    ).unwrap_or(0.0)
}

pub fn cost_by_model(db: &Db) -> Vec<(String, f64)> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT routed_model, SUM(cost_usd) FROM api_calls
         WHERE strftime('%Y-%m', timestamp) = strftime('%Y-%m','now')
         GROUP BY routed_model ORDER BY SUM(cost_usd) DESC"
    ).unwrap();
    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))
        .unwrap().filter_map(|x| x.ok()).collect()
}

pub fn daily_cost(db: &Db, days: u32) -> Vec<(String, f64)> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(&format!(
        "SELECT date(timestamp) as day, SUM(cost_usd) as total
         FROM api_calls WHERE timestamp >= datetime('now','-{days} days')
         GROUP BY day ORDER BY day"
    )).unwrap();
    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))
        .unwrap().filter_map(|x| x.ok()).collect()
}

pub fn avg_latency(db: &Db) -> Vec<(String, f64, f64)> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT routed_model, AVG(latency_ms), MAX(latency_ms)
         FROM api_calls
         WHERE strftime('%Y-%m', timestamp) = strftime('%Y-%m','now')
         GROUP BY routed_model"
    ).unwrap();
    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?)))
        .unwrap().filter_map(|x| x.ok()).collect()
}

pub fn routing_distribution(db: &Db) -> Vec<(String, i64, f64)> {
    let conn = db.lock().unwrap();
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM api_calls", [], |r| r.get(0)).unwrap_or(0);
    if total == 0 { return vec![]; }
    let mut stmt = conn.prepare(
        "SELECT routed_model, COUNT(*) FROM api_calls GROUP BY routed_model ORDER BY COUNT(*) DESC"
    ).unwrap();
    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .unwrap().filter_map(|x| x.ok())
        .map(|(m, c)| { let pct = (c as f64 / total as f64) * 100.0; (m, c, pct) })
        .collect()
}

pub fn cache_hit_rate(db: &Db) -> f64 {
    let conn = db.lock().unwrap();
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM api_calls", [], |r| r.get(0)).unwrap_or(0);
    if total == 0 { return 0.0; }
    let hits: i64 = conn.query_row("SELECT COUNT(*) FROM api_calls WHERE cache_hit=1", [], |r| r.get(0)).unwrap_or(0);
    (hits as f64 / total as f64) * 100.0
}

pub fn recent_calls(db: &Db, limit: u32) -> Vec<serde_json::Value> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(&format!(
        "SELECT request_id, routed_model, task_type, complexity,
                input_tokens, output_tokens, cost_usd, latency_ms,
                cache_hit, timestamp
         FROM api_calls ORDER BY id DESC LIMIT {limit}"
    )).unwrap();
    stmt.query_map([], |r| Ok(serde_json::json!({
        "request_id":    r.get::<_, String>(0)?,
        "routed_model":  r.get::<_, String>(1)?,
        "task_type":     r.get::<_, String>(2)?,
        "complexity":    r.get::<_, String>(3)?,
        "input_tokens":  r.get::<_, i64>(4)?,
        "output_tokens": r.get::<_, i64>(5)?,
        "cost_usd":      r.get::<_, f64>(6)?,
        "latency_ms":    r.get::<_, i64>(7)?,
        "cache_hit":     r.get::<_, bool>(8)?,
        "timestamp":     r.get::<_, String>(9)?,
    }))).unwrap().filter_map(|x| x.ok()).collect()
}

pub fn export_calls_to_csv(db: &Db) -> Result<String, rusqlite::Error> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT request_id, timestamp, routed_model, task_type, complexity,
                input_tokens, output_tokens, cost_usd, latency_ms, cache_hit
         FROM api_calls ORDER BY id DESC"
    )?;
    
    let mut csv = String::from("request_id,timestamp,model,task_type,complexity,input_tokens,output_tokens,cost_usd,latency_ms,cache_hit\n");
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let request_id: String = row.get(0)?;
        let timestamp: String = row.get(1)?;
        let model: String = row.get(2)?;
        let task_type: String = row.get(3)?;
        let complexity: String = row.get(4)?;
        let input_tokens: i64 = row.get(5)?;
        let output_tokens: i64 = row.get(6)?;
        let cost_usd: f64 = row.get(7)?;
        let latency_ms: i64 = row.get(8)?;
        let cache_hit: i64 = row.get(9)?;
        
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{:.6},{},{}\n",
            request_id, timestamp, model, task_type, complexity,
            input_tokens, output_tokens, cost_usd, latency_ms, cache_hit
        ));
    }
    Ok(csv)
}

