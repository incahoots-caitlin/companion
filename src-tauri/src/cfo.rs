// Companion - Studio CFO (v0.37 Block F).
//
// Read-only financial intelligence aggregated from existing Airtable
// tables (TimeLogs, Projects, Receipts, Clients). Nothing here writes
// to Airtable. Each command runs filterByFormula queries with month
// boundaries, then aggregates in Rust.
//
// Caitlin is the studio principal (rate $110/h). Rose is the only
// subcontractor (rate $66/h). When a TimeLog has its `rate` field set
// we trust it; otherwise we fall back to the standard subcontractor
// rate based on subcontractor_code. Hours without a sub code are
// assumed to be Caitlin's.
//
// Hour creep alerts surface any project where logged_hours / budgeted_hours
// is over 80%. Budgeted hours come from Projects.budget_total divided by
// Caitlin's $110/h (a rough proxy — projects with explicit budget_hours
// would be cleaner, but the table doesn't have that field today).

use crate::{airtable_get, urlencode};
use serde::Serialize;

const TIMELOGS_TABLE: &str = "TimeLogs";
const PROJECTS_TABLE: &str = "Projects";
const CLIENTS_TABLE: &str = "Clients";

const CAITLIN_RATE: f64 = 110.0;
const ROSE_RATE: f64 = 66.0;
const HOUR_CREEP_THRESHOLD: f64 = 0.80;

// ── Public types ──────────────────────────────────────────────────────

#[derive(Serialize, Debug, Clone, Default)]
pub struct StudioTotals {
    pub year: i32,
    pub month: u32,
    pub hours_total: f64,
    pub hours_billable: f64,
    pub hours_internal: f64,
    pub hours_caitlin: f64,
    pub hours_rose: f64,
    pub revenue: f64,
    pub subcontractor_cost: f64,
    pub margin: f64,
    pub avg_margin_per_hour: f64,
}

#[derive(Serialize, Debug, Clone)]
pub struct ClientFinancials {
    pub client_code: String,
    pub client_name: Option<String>,
    pub hours: f64,
    pub hours_billable: f64,
    pub revenue: f64,
    pub budget_total: Option<f64>,
    pub budget_burn_pct: Option<f64>,
    pub no_budget: bool,
    pub internal: bool,
}

#[derive(Serialize, Debug, Clone)]
pub struct HourCreepAlert {
    pub project_code: String,
    pub project_name: Option<String>,
    pub client_code: Option<String>,
    pub budget_total: f64,
    pub hours_logged: f64,
    pub hours_budgeted: f64,
    pub burn_pct: f64,
    pub end_date: Option<String>,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct NextMonthOutlook {
    pub year: i32,
    pub month: u32,
    pub active_projects: u32,
    pub budgeted_total: f64,
    pub capacity_caitlin_hours: f64,
    pub capacity_rose_hours: f64,
}

// ── Internal helpers ──────────────────────────────────────────────────

fn month_bounds(year: i32, month: u32) -> (String, String) {
    // Inclusive start (YYYY-MM-01), exclusive end (next month YYYY-MM-01).
    let start = format!("{:04}-{:02}-01", year, month);
    let (ny, nm) = if month >= 12 {
        (year + 1, 1u32)
    } else {
        (year, month + 1)
    };
    let end = format!("{:04}-{:02}-01", ny, nm);
    (start, end)
}

fn rate_for_log(log_fields: &serde_json::Value) -> f64 {
    // Prefer the rate stamped on the row.
    if let Some(r) = log_fields["rate"].as_f64() {
        if r > 0.0 {
            return r;
        }
    }
    // Subcontractor-coded rows are Rose at the moment. Treat anything
    // else (including no sub link) as Caitlin's hours.
    let sub = log_fields["subcontractor_code"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !sub.is_empty() {
        ROSE_RATE
    } else {
        CAITLIN_RATE
    }
}

fn is_rose(log_fields: &serde_json::Value) -> bool {
    // sub field on a TimeLog can be an array of record ids (the link),
    // an array of codes (rollup), or empty. We only need a yes/no.
    let from_lookup = log_fields["subcontractor_code"]
        .as_array()
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);
    let from_link = log_fields["subcontractor"]
        .as_array()
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);
    from_lookup || from_link
}

fn first_str(value: &serde_json::Value) -> Option<String> {
    value
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .map(String::from)
}

async fn fetch_timelogs_for_month(year: i32, month: u32) -> Result<serde_json::Value, String> {
    let (start, end) = month_bounds(year, month);
    // Airtable date comparison: IS_AFTER / IS_BEFORE accept ISO strings.
    let formula = format!(
        "AND(IS_AFTER({{date}},'{}'),IS_BEFORE({{date}},'{}'))",
        // IS_AFTER is strict, so step the start back one day.
        step_day(&start, -1),
        end
    );
    let qs = format!(
        "filterByFormula={}\
&fields%5B%5D=date\
&fields%5B%5D=hours\
&fields%5B%5D=billable\
&fields%5B%5D=rate\
&fields%5B%5D=client\
&fields%5B%5D=client_code\
&fields%5B%5D=project\
&fields%5B%5D=project_code\
&fields%5B%5D=subcontractor\
&fields%5B%5D=subcontractor_code",
        urlencode(&formula)
    );
    airtable_get(TIMELOGS_TABLE, &qs).await
}

fn step_day(iso: &str, days: i32) -> String {
    // Tiny helper: shift a YYYY-MM-DD by N days using chrono.
    let d = chrono::NaiveDate::parse_from_str(iso, "%Y-%m-%d").unwrap_or_else(|_| {
        chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("valid fallback date")
    });
    let shifted = d
        .checked_add_signed(chrono::Duration::days(days as i64))
        .unwrap_or(d);
    shifted.format("%Y-%m-%d").to_string()
}

async fn fetch_active_projects() -> Result<serde_json::Value, String> {
    let qs = "filterByFormula=NOT(%7Bstatus%7D%3D%27archive%27)\
&fields%5B%5D=code\
&fields%5B%5D=name\
&fields%5B%5D=status\
&fields%5B%5D=client\
&fields%5B%5D=client_code\
&fields%5B%5D=start_date\
&fields%5B%5D=end_date\
&fields%5B%5D=budget_total";
    airtable_get(PROJECTS_TABLE, qs).await
}

async fn fetch_clients_lookup() -> Result<std::collections::HashMap<String, (String, String)>, String>
{
    // Returns record_id -> (code, name)
    let data = airtable_get(
        CLIENTS_TABLE,
        "fields%5B%5D=code&fields%5B%5D=name&fields%5B%5D=status",
    )
    .await?;
    let mut out = std::collections::HashMap::new();
    if let Some(records) = data["records"].as_array() {
        for r in records {
            let id = r["id"].as_str().unwrap_or("").to_string();
            let code = r["fields"]["code"].as_str().unwrap_or("").to_string();
            let name = r["fields"]["name"].as_str().unwrap_or("").to_string();
            if !id.is_empty() && !code.is_empty() {
                out.insert(id, (code, name));
            }
        }
    }
    Ok(out)
}

fn resolve_client_code(
    log_fields: &serde_json::Value,
    clients: &std::collections::HashMap<String, (String, String)>,
) -> Option<(String, String)> {
    // Lookup field rollup first.
    if let Some(code) = first_str(&log_fields["client_code"]) {
        let upper = code.trim().to_uppercase();
        if !upper.is_empty() {
            // Try to enrich with the canonical name from the Clients map.
            for (_, (c, n)) in clients.iter() {
                if c.eq_ignore_ascii_case(&upper) {
                    return Some((upper, n.clone()));
                }
            }
            return Some((upper, String::new()));
        }
    }
    // Fallback to record id link.
    if let Some(arr) = log_fields["client"].as_array() {
        if let Some(first) = arr.first().and_then(|v| v.as_str()) {
            if let Some((code, name)) = clients.get(first) {
                return Some((code.clone(), name.clone()));
            }
        }
    }
    None
}

// ── Public API ────────────────────────────────────────────────────────

pub async fn studio_totals(year: i32, month: u32) -> Result<StudioTotals, String> {
    let logs_data = fetch_timelogs_for_month(year, month).await?;
    let mut totals = StudioTotals {
        year,
        month,
        ..Default::default()
    };
    if let Some(records) = logs_data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let hours = f["hours"].as_f64().unwrap_or(0.0);
            if hours <= 0.0 {
                continue;
            }
            let billable = f["billable"].as_bool().unwrap_or(true);
            let rate = rate_for_log(f);
            let rose = is_rose(f);

            totals.hours_total += hours;
            if billable {
                totals.hours_billable += hours;
                totals.revenue += hours * rate;
            } else {
                totals.hours_internal += hours;
            }
            if rose {
                totals.hours_rose += hours;
                totals.subcontractor_cost += hours * ROSE_RATE;
            } else {
                totals.hours_caitlin += hours;
            }
        }
    }
    totals.margin = totals.revenue - totals.subcontractor_cost;
    if totals.hours_billable > 0.0 {
        totals.avg_margin_per_hour = totals.margin / totals.hours_billable;
    }
    Ok(totals)
}

pub async fn per_client(year: i32, month: u32) -> Result<Vec<ClientFinancials>, String> {
    let clients = fetch_clients_lookup().await.unwrap_or_default();
    let logs_data = fetch_timelogs_for_month(year, month).await?;
    let projects_data = fetch_active_projects().await.unwrap_or(serde_json::Value::Null);

    // Aggregate hours and revenue by client code.
    let mut by_client: std::collections::HashMap<String, ClientFinancials> =
        std::collections::HashMap::new();

    if let Some(records) = logs_data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let hours = f["hours"].as_f64().unwrap_or(0.0);
            if hours <= 0.0 {
                continue;
            }
            let billable = f["billable"].as_bool().unwrap_or(true);
            let rate = rate_for_log(f);
            let (code, name) = resolve_client_code(f, &clients).unwrap_or_else(|| {
                ("INC".to_string(), "In Cahoots (internal)".to_string())
            });
            let entry = by_client.entry(code.clone()).or_insert(ClientFinancials {
                client_code: code.clone(),
                client_name: if name.is_empty() { None } else { Some(name) },
                hours: 0.0,
                hours_billable: 0.0,
                revenue: 0.0,
                budget_total: None,
                budget_burn_pct: None,
                no_budget: true,
                internal: code == "INC",
            });
            entry.hours += hours;
            if billable {
                entry.hours_billable += hours;
                entry.revenue += hours * rate;
            }
        }
    }

    // Sum active project budget per client (in this month).
    let (start, end) = month_bounds(year, month);
    if let Some(records) = projects_data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let budget = f["budget_total"].as_f64().unwrap_or(0.0);
            if budget <= 0.0 {
                continue;
            }
            // Active in this month: project's start <= end-of-month and
            // (no end OR end >= start-of-month).
            let proj_start = f["start_date"].as_str().unwrap_or("");
            let proj_end = f["end_date"].as_str().unwrap_or("");
            if !proj_start.is_empty() && proj_start >= end.as_str() {
                continue;
            }
            if !proj_end.is_empty() && proj_end < start.as_str() {
                continue;
            }
            let code = first_str(&f["client_code"])
                .map(|s| s.trim().to_uppercase())
                .or_else(|| {
                    f["client"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|v| v.as_str())
                        .and_then(|id| clients.get(id).map(|(c, _)| c.clone()))
                });
            if let Some(code) = code {
                let entry = by_client.entry(code.clone()).or_insert(ClientFinancials {
                    client_code: code.clone(),
                    client_name: clients
                        .values()
                        .find(|(c, _)| c.eq_ignore_ascii_case(&code))
                        .map(|(_, n)| n.clone()),
                    hours: 0.0,
                    hours_billable: 0.0,
                    revenue: 0.0,
                    budget_total: None,
                    budget_burn_pct: None,
                    no_budget: true,
                    internal: false,
                });
                entry.budget_total = Some(entry.budget_total.unwrap_or(0.0) + budget);
                entry.no_budget = false;
            }
        }
    }

    // Compute burn percentage where we have both numbers.
    for c in by_client.values_mut() {
        if let Some(bt) = c.budget_total {
            if bt > 0.0 {
                c.budget_burn_pct = Some(c.revenue / bt);
            }
        }
    }

    let mut out: Vec<ClientFinancials> = by_client.into_values().collect();
    // Sort: internal last, otherwise by revenue desc.
    out.sort_by(|a, b| {
        a.internal
            .cmp(&b.internal)
            .then_with(|| b.revenue.partial_cmp(&a.revenue).unwrap_or(std::cmp::Ordering::Equal))
    });
    Ok(out)
}

pub async fn hour_creep_alerts() -> Result<Vec<HourCreepAlert>, String> {
    // For every active project with a budget > 0, sum all-time logged
    // hours and compare against budgeted hours (budget / Caitlin's rate
    // as a proxy). Surface anything over the threshold.
    let clients = fetch_clients_lookup().await.unwrap_or_default();
    let projects_data = fetch_active_projects().await?;
    // Pull all TimeLogs (no date filter — hour creep is lifetime per
    // project). Capped via maxRecords + pagination if it gets large.
    let logs_data = airtable_get(
        TIMELOGS_TABLE,
        "fields%5B%5D=hours\
&fields%5B%5D=project\
&fields%5B%5D=project_code\
&fields%5B%5D=billable",
    )
    .await
    .unwrap_or(serde_json::Value::Null);

    // hours by project code
    let mut hours_by_project: std::collections::HashMap<String, f64> =
        std::collections::HashMap::new();
    if let Some(records) = logs_data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let h = f["hours"].as_f64().unwrap_or(0.0);
            if h <= 0.0 {
                continue;
            }
            let pc = first_str(&f["project_code"]);
            if let Some(code) = pc {
                let key = code.trim().to_string();
                if !key.is_empty() {
                    *hours_by_project.entry(key).or_insert(0.0) += h;
                }
            }
        }
    }

    let mut out: Vec<HourCreepAlert> = Vec::new();
    if let Some(records) = projects_data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let budget = f["budget_total"].as_f64().unwrap_or(0.0);
            if budget <= 0.0 {
                continue;
            }
            let code = match f["code"].as_str() {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            };
            let logged = hours_by_project.get(&code).copied().unwrap_or(0.0);
            let budgeted = budget / CAITLIN_RATE;
            if budgeted <= 0.0 {
                continue;
            }
            let pct = logged / budgeted;
            if pct < HOUR_CREEP_THRESHOLD {
                continue;
            }
            let client_code = first_str(&f["client_code"])
                .map(|s| s.trim().to_uppercase())
                .or_else(|| {
                    f["client"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|v| v.as_str())
                        .and_then(|id| clients.get(id).map(|(c, _)| c.clone()))
                });
            out.push(HourCreepAlert {
                project_code: code,
                project_name: f["name"].as_str().map(String::from),
                client_code,
                budget_total: budget,
                hours_logged: logged,
                hours_budgeted: budgeted,
                burn_pct: pct,
                end_date: f["end_date"].as_str().map(String::from),
            });
        }
    }
    out.sort_by(|a, b| b.burn_pct.partial_cmp(&a.burn_pct).unwrap_or(std::cmp::Ordering::Equal));
    Ok(out)
}

pub async fn outlook(year: i32, month: u32) -> Result<NextMonthOutlook, String> {
    // "Next month" means the month *after* the (year, month) the caller
    // passed (i.e. May -> June). Caitlin's working capacity is treated
    // as 75% of a 160h month for her, 50% of a 80h month for Rose
    // (rough but matches the Studio architecture plan).
    let (ny, nm) = if month >= 12 {
        (year + 1, 1u32)
    } else {
        (year, month + 1)
    };
    let (start, end) = month_bounds(ny, nm);
    let projects_data = fetch_active_projects().await?;
    let mut active_projects = 0u32;
    let mut budgeted_total = 0.0;
    if let Some(records) = projects_data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let proj_start = f["start_date"].as_str().unwrap_or("");
            let proj_end = f["end_date"].as_str().unwrap_or("");
            if !proj_start.is_empty() && proj_start >= end.as_str() {
                continue;
            }
            if !proj_end.is_empty() && proj_end < start.as_str() {
                continue;
            }
            active_projects += 1;
            budgeted_total += f["budget_total"].as_f64().unwrap_or(0.0);
        }
    }
    Ok(NextMonthOutlook {
        year: ny,
        month: nm,
        active_projects,
        budgeted_total,
        capacity_caitlin_hours: 160.0 * 0.75,
        capacity_rose_hours: 80.0 * 0.50,
    })
}
