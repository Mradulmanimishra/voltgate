use crate::database::*;

/// Escape HTML special characters to prevent XSS when embedding
/// database values into the dashboard HTML template.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&#x27;")
}

pub fn render(db: &Db) -> String {
    let today      = cost_today(db);
    let by_model   = cost_by_model(db);
    let daily      = daily_cost(db, 14);
    let latency    = avg_latency(db);
    let routing    = routing_distribution(db);
    let cache_rate = cache_hit_rate(db);
    let recent     = recent_calls(db, 20);

    let daily_labels: Vec<String> = daily.iter().map(|(d, _)| format!("\"{d}\"")).collect();
    let daily_values: Vec<String> = daily.iter().map(|(_, v)| format!("{v:.4}")).collect();
    let model_labels: Vec<String> = routing.iter().map(|(m, _, _)| { let s = m.split('-').next().unwrap_or(m); format!("\"{s}\"") }).collect();
    let model_pcts: Vec<String>   = routing.iter().map(|(_, _, p)| format!("{p:.1}")).collect();

    let recent_rows = recent.iter().map(|r| {
        let model  = r["routed_model"].as_str().unwrap_or("");
        let short  = model.split('-').next().unwrap_or(model);
        let comp   = r["complexity"].as_str().unwrap_or("");
        let cost   = r["cost_usd"].as_f64().unwrap_or(0.0);
        let lat    = r["latency_ms"].as_i64().unwrap_or(0);
        let cached = r["cache_hit"].as_bool().unwrap_or(false);
        let ts     = r["timestamp"].as_str().unwrap_or("").get(..16).unwrap_or("");
        format!("<tr><td>{}</td><td>{}</td><td>{}</td><td>${:.6}</td><td>{}ms</td><td>{}</td></tr>", esc(ts), esc(short), esc(comp), cost, lat, if cached { "✓" } else { "" })
    }).collect::<Vec<_>>().join("\n");

    let lat_rows = latency.iter().map(|(m, avg, max)| {
        let short = m.split('-').next().unwrap_or(m);
        format!("<tr><td>{}</td><td>{:.0}ms</td><td>{:.0}ms</td></tr>", esc(short), avg, max)
    }).collect::<Vec<_>>().join("\n");

    let model_rows = by_model.iter().map(|(m, cost)| {
        let short = m.split('-').next().unwrap_or(m);
        format!("<tr><td>{}</td><td>${:.4}</td></tr>", esc(short), cost)
    }).collect::<Vec<_>>().join("\n");

    format!(r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>VoltGate Dashboard</title>
<script src="https://cdnjs.cloudflare.com/ajax/libs/Chart.js/4.4.0/chart.umd.min.js"></script>
<style>
*{{box-sizing:border-box;margin:0;padding:0}}
body{{font-family:system-ui,sans-serif;background:#0f1117;color:#e2e8f0;padding:1.5rem}}
h1{{font-size:20px;font-weight:600;margin-bottom:.25rem}}
.sub{{font-size:13px;color:#64748b;margin-bottom:1.5rem}}
.grid-4{{display:grid;grid-template-columns:repeat(4,1fr);gap:.75rem;margin-bottom:1.25rem}}
.card{{background:#1e2130;border:1px solid #2d3148;border-radius:10px;padding:1rem 1.25rem}}
.card-label{{font-size:11px;color:#64748b;text-transform:uppercase;letter-spacing:.5px}}
.card-value{{font-size:26px;font-weight:700;margin-top:.3rem}}
.charts-row{{display:grid;grid-template-columns:2fr 1fr;gap:.75rem;margin-bottom:1.25rem}}
table{{width:100%;border-collapse:collapse;font-size:12px}}
th,td{{padding:.45rem .75rem;text-align:left;border-bottom:1px solid #2d3148}}
th{{color:#64748b;font-weight:500;font-size:11px;text-transform:uppercase}}
tr:last-child td{{border:none}}
tr:hover td{{background:#252840}}
.green{{color:#22c55e}}.blue{{color:#6366f1}}

/* Modal style */
.modal-overlay{{display:none;position:fixed;top:0;left:0;width:100%;height:100%;background:rgba(15,17,23,0.85);backdrop-filter:blur(4px);justify-content:center;align-items:center;z-index:1000}}
.modal{{background:#1e2130;border:1px solid #2d3148;border-radius:12px;width:100%;max-width:550px;padding:1.5rem;box-shadow:0 10px 30px rgba(0,0,0,0.5)}}
.modal-header{{display:flex;justify-content:space-between;align-items:center;margin-bottom:1.25rem}}
.modal-title{{font-size:16px;font-weight:600}}
.close-btn{{background:none;border:none;color:#64748b;font-size:20px;cursor:pointer}}
.close-btn:hover{{color:#f43f5e}}
.form-group{{margin-bottom:1rem}}
.form-group label{{display:block;font-size:11px;color:#94a3b8;text-transform:uppercase;margin-bottom:.35rem}}
.form-control{{width:100%;background:#0f1117;border:1px solid #2d3148;border-radius:6px;padding:.5rem .75rem;color:#e2e8f0;font-size:13px}}
.form-control:focus{{outline:none;border-color:#6366f1}}
.btn{{background:#6366f1;color:#fff;border:none;border-radius:6px;padding:.5rem 1rem;cursor:pointer;font-size:13px;font-weight:500}}
.btn:hover{{background:#4f46e5}}
.btn-sec{{background:#1e2130;border:1px solid #2d3148;color:#94a3b8;border-radius:6px;padding:.4rem .9rem;cursor:pointer;font-size:12px}}
.btn-sec:hover{{color:#e2e8f0;background:#252840}}
</style></head>
<body>
<div style="display:flex;justify-content:space-between;align-items:flex-start">
  <div><h1>⚡ VoltGate Dashboard</h1><p class="sub">Real-time routing decisions · cost tracking · latency</p></div>
  <div style="display:flex;gap:.5rem">
    <button onclick="downloadCSV()" class="btn-sec">📥 Export CSV</button>
    <button onclick="openSettings()" class="btn-sec">⚙️ Settings</button>
    <button onclick="location.reload()" class="btn-sec">↻ Refresh</button>
  </div>
</div>
<div class="grid-4">
  <div class="card"><div class="card-label">Cost today</div><div class="card-value green">${today:.4}</div></div>
  <div class="card"><div class="card-label">Cache hit rate</div><div class="card-value blue">{cache_rate:.1}%</div></div>
  <div class="card"><div class="card-label">Models active</div><div class="card-value">{model_count}</div></div>
  <div class="card"><div class="card-label">Total routes</div><div class="card-value">{total_routes}</div></div>
</div>
<div class="charts-row">
  <div class="card"><div class="card-label" style="margin-bottom:.75rem">Daily cost — last 14 days</div><canvas id="dailyChart" height="80"></canvas></div>
  <div class="card"><div class="card-label" style="margin-bottom:.75rem">Routing distribution</div><canvas id="routingChart" height="160"></canvas></div>
</div>
<div style="display:grid;grid-template-columns:1fr 1fr;gap:.75rem;margin-bottom:1.25rem">
  <div class="card"><div class="card-label" style="margin-bottom:.75rem">Cost by model (this month)</div><table><thead><tr><th>Model</th><th>Cost</th></tr></thead><tbody>{model_rows}</tbody></table></div>
  <div class="card"><div class="card-label" style="margin-bottom:.75rem">Average latency by model</div><table><thead><tr><th>Model</th><th>Avg</th><th>Max</th></tr></thead><tbody>{lat_rows}</tbody></table></div>
</div>
<div class="card"><div class="card-label" style="margin-bottom:.75rem">Recent requests</div>
<table><thead><tr><th>Time</th><th>Model</th><th>Complexity</th><th>Cost</th><th>Latency</th><th>Cached</th></tr></thead><tbody>{recent_rows}</tbody></table></div>

<!-- Settings Modal -->
<div id="settingsModal" class="modal-overlay">
  <div class="modal">
    <div class="modal-header">
      <div class="modal-title">⚙️ Guardrails & Budgets Config</div>
      <button onclick="closeSettings()" class="close-btn">&times;</button>
    </div>
    <div class="form-group">
      <label>Router API Key</label>
      <input type="password" id="cfgApiKey" class="form-control" placeholder="Enter bearer router key">
    </div>
    <div style="display:grid;grid-template-columns:1fr 1fr;gap:.75rem">
      <div class="form-group">
        <label>Max Cost Per Request (USD)</label>
        <input type="number" step="0.01" id="cfgMaxCost" class="form-control">
      </div>
      <div class="form-group">
        <label>Max Tokens Per Request</label>
        <input type="number" id="cfgMaxTokens" class="form-control">
      </div>
    </div>
    <div class="form-group">
      <label>Blocked Phrases (comma-separated)</label>
      <textarea id="cfgBlocked" class="form-control" rows="2" style="resize:vertical"></textarea>
    </div>
    <div class="form-group">
      <label>Force Model (Overrides Routing)</label>
      <select id="cfgForceModel" class="form-control">
        <option value="">None (Auto Routing)</option>
        <option value="claude-haiku-4-5">claude-haiku-4-5</option>
        <option value="claude-sonnet-4-6">claude-sonnet-4-6</option>
        <option value="claude-opus-4-8">claude-opus-4-8</option>
        <option value="claude-fable-5">claude-fable-5</option>
      </select>
    </div>
    <div style="border-top:1px solid #2d3148;padding-top:1rem;margin-top:1rem;display:flex;justify-content:space-between;align-items:center">
      <label style="display:flex;align-items:center;font-size:13px;color:#94a3b8;cursor:pointer">
        <input type="checkbox" id="cfgBudgetsEnabled" style="margin-right:.5rem"> Enable Model budgets
      </label>
      <div style="display:flex;align-items:center;gap:.5rem">
        <label style="font-size:11px;color:#64748b">On budget exceeded</label>
        <select id="cfgBudgetsAction" class="form-control" style="width:110px">
          <option value="fallback">fallback</option>
          <option value="reject">reject</option>
        </select>
      </div>
    </div>
    <div style="margin-top:1.5rem;display:flex;justify-content:flex-end;gap:.5rem">
      <button onclick="closeSettings()" class="btn-sec" style="padding:.5rem 1rem">Cancel</button>
      <button onclick="saveSettings()" class="btn">Save Config</button>
    </div>
  </div>
</div>

<script>
new Chart(document.getElementById('dailyChart'), {{ type: 'line', data: {{ labels: [{daily_labels}], datasets: [{{ label: 'Cost ($)', data: [{daily_values}], borderColor: '#6366f1', backgroundColor: 'rgba(99,102,241,0.08)', tension: 0.3, fill: true, pointRadius: 3, pointBackgroundColor: '#6366f1' }}] }}, options: {{ plugins: {{ legend: {{ display: false }} }}, scales: {{ x: {{ ticks: {{ color:'#64748b', maxTicksLimit:7 }}, grid: {{ color:'#2d3148' }} }}, y: {{ ticks: {{ color:'#64748b', callback: v => '$'+v.toFixed(3) }}, grid: {{ color:'#2d3148' }} }} }} }} }});
new Chart(document.getElementById('routingChart'), {{ type: 'doughnut', data: {{ labels: [{model_labels}], datasets: [{{ data: [{model_pcts}], backgroundColor: ['#22c55e','#6366f1','#f59e0b','#ef4444'], borderWidth: 0 }}] }}, options: {{ plugins: {{ legend: {{ labels: {{ color:'#94a3b8', padding:14, font:{{ size:11 }} }} }} }}, cutout: '60%' }} }});

// Settings & CSV Actions
function getAuthHeaders(key) {{
    return {{
        'Authorization': 'Bearer ' + key,
        'Content-Type': 'application/json'
    }};
}}

async function downloadCSV() {{
    let key = localStorage.getItem('router_api_key') || prompt('Enter Router API Key:');
    if (!key) return;
    localStorage.setItem('router_api_key', key);
    try {{
        let resp = await fetch('/api/stats/csv', {{ headers: getAuthHeaders(key) }});
        if (!resp.ok) {{ alert('Download failed: ' + resp.statusText); return; }}
        let blob = await resp.blob();
        let url = window.URL.createObjectURL(blob);
        let a = document.createElement('a');
        a.href = url;
        a.download = 'cost_report.csv';
        document.body.appendChild(a);
        a.click();
        a.remove();
    }} catch (e) {{
        alert('Network error downloading CSV: ' + e);
    }}
}}

async function openSettings() {{
    let key = localStorage.getItem('router_api_key') || prompt('Enter Router API Key:');
    if (!key) return;
    localStorage.setItem('router_api_key', key);
    document.getElementById('cfgApiKey').value = key;
    
    document.getElementById('settingsModal').style.display = 'flex';
    try {{
        let resp = await fetch('/api/config', {{ headers: getAuthHeaders(key) }});
        if (!resp.ok) {{
            alert('Failed to load configuration: ' + resp.statusText);
            closeSettings();
            return;
        }}
        let data = await resp.json();
        document.getElementById('cfgMaxCost').value = data.guardrails.max_cost_per_request_usd;
        document.getElementById('cfgMaxTokens').value = data.guardrails.max_tokens_per_request;
        document.getElementById('cfgBlocked').value = data.guardrails.blocked_phrases.join(', ');
        document.getElementById('cfgForceModel').value = data.guardrails.force_model || '';
        document.getElementById('cfgBudgetsEnabled').checked = data.model_budgets.enabled;
        document.getElementById('cfgBudgetsAction').value = data.model_budgets.on_exceeded;
    }} catch (e) {{
        alert('Error loading configuration: ' + e);
        closeSettings();
    }}
}}

function closeSettings() {{
    document.getElementById('settingsModal').style.display = 'none';
}}

async function saveSettings() {{
    let key = document.getElementById('cfgApiKey').value;
    if (!key) {{ alert('Router API Key is required to save'); return; }}
    localStorage.setItem('router_api_key', key);
    
    let blocked_phrases = document.getElementById('cfgBlocked').value
        .split(',')
        .map(s => s.trim())
        .filter(s => s.length > 0);
        
    let force_model = document.getElementById('cfgForceModel').value;
    
    let body = {{
        guardrails: {{
            max_cost_per_request_usd: parseFloat(document.getElementById('cfgMaxCost').value),
            max_tokens_per_request: parseInt(document.getElementById('cfgMaxTokens').value),
            blocked_phrases: blocked_phrases,
            force_model: force_model ? force_model : null
        }},
        model_budgets: {{
            enabled: document.getElementById('cfgBudgetsEnabled').checked,
            on_exceeded: document.getElementById('cfgBudgetsAction').value,
            daily_limits_usd: {{
                'claude-fable-5': 50.0,
                'claude-opus-4-8': 30.0,
                'claude-sonnet-4-6': 100.0,
                'claude-haiku-4-5': 20.0
            }}
        }}
    }};
    
    try {{
        let resp = await fetch('/api/config', {{
            method: 'POST',
            headers: getAuthHeaders(key),
            body: JSON.stringify(body)
        }});
        let data = await resp.json();
        if (resp.ok) {{
            alert(data.message || 'Config saved successfully!');
            closeSettings();
        }} else {{
            alert('Save failed: ' + (data.error || resp.statusText));
        }}
    }} catch (e) {{
        alert('Error saving config: ' + e);
    }}
}}
</script>
</body></html>"#,
        today = today, cache_rate = cache_rate, model_count = by_model.len(),
        total_routes = routing.iter().map(|(_, c, _)| c).sum::<i64>(),
        model_rows = model_rows, lat_rows = lat_rows, recent_rows = recent_rows,
        daily_labels = daily_labels.join(","), daily_values = daily_values.join(","),
        model_labels = model_labels.join(","), model_pcts = model_pcts.join(","),
    )
}
