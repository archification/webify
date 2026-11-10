// Live server metrics for the public status page. A middleware layer on the
// whole app records every request's status and latency into process-wide
// counters; the status mount ("/status" = ["static/status.html", "status"] in
// config.toml) serves a page plus a JSON snapshot the page polls.
//
// Nothing per-visitor is kept: only counts, latencies and per-minute totals.

use axum::{
    Extension, Json, Router,
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::Utc;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::AppState;

/// Latency percentiles are computed over this trailing window.
const WINDOW: Duration = Duration::from_secs(60);
/// Hard cap on stored latency samples so a traffic spike can't grow memory.
const MAX_SAMPLES: usize = 100_000;
/// Minutes of per-minute request totals kept for the history chart.
const HISTORY_MINUTES: i64 = 60;

struct Inner {
    /// (when, latency in microseconds) for requests inside `WINDOW`.
    samples: VecDeque<(Instant, u32)>,
    /// (unix minute, requests, 5xx responses), oldest first.
    minutes: VecDeque<(i64, u64, u64)>,
}

struct Metrics {
    started: Instant,
    started_at: i64,
    total: AtomicU64,
    /// Responses by status class: 1xx..5xx.
    classes: [AtomicU64; 5],
    in_flight: AtomicI64,
    inner: Mutex<Inner>,
}

static METRICS: LazyLock<Metrics> = LazyLock::new(|| Metrics {
    started: Instant::now(),
    started_at: Utc::now().timestamp(),
    total: AtomicU64::new(0),
    classes: Default::default(),
    in_flight: AtomicI64::new(0),
    inner: Mutex::new(Inner { samples: VecDeque::new(), minutes: VecDeque::new() }),
});

/// Response extension marking a request that shouldn't be counted (the status
/// page's own polling would otherwise dominate a quiet site's numbers).
#[derive(Clone)]
struct Untracked;

/// Middleware: count the request and its latency.
pub async fn track(req: Request, next: Next) -> Response {
    let m = &*METRICS;
    m.in_flight.fetch_add(1, Ordering::Relaxed);
    let t0 = Instant::now();
    let resp = next.run(req).await;
    let elapsed = t0.elapsed();
    m.in_flight.fetch_sub(1, Ordering::Relaxed);
    if resp.extensions().get::<Untracked>().is_none() {
        record(resp.status().as_u16(), elapsed);
    }
    resp
}

fn record(status: u16, elapsed: Duration) {
    let m = &*METRICS;
    m.total.fetch_add(1, Ordering::Relaxed);
    if let Some(c) = m.classes.get((status / 100).saturating_sub(1) as usize) {
        c.fetch_add(1, Ordering::Relaxed);
    }
    let now = Instant::now();
    let minute = Utc::now().timestamp() / 60;
    let micros = elapsed.as_micros().min(u32::MAX as u128) as u32;
    let mut inner = m.inner.lock().unwrap();
    prune(&mut inner, now, minute);
    if inner.samples.len() >= MAX_SAMPLES {
        inner.samples.pop_front();
    }
    inner.samples.push_back((now, micros));
    match inner.minutes.back_mut() {
        Some(last) if last.0 == minute => {
            last.1 += 1;
            last.2 += (status >= 500) as u64;
        }
        _ => inner.minutes.push_back((minute, 1, (status >= 500) as u64)),
    }
}

fn prune(inner: &mut Inner, now: Instant, minute: i64) {
    while inner.samples.front().is_some_and(|(t, _)| now.duration_since(*t) > WINDOW) {
        inner.samples.pop_front();
    }
    while inner.minutes.front().is_some_and(|(m, _, _)| *m <= minute - HISTORY_MINUTES) {
        inner.minutes.pop_front();
    }
}

#[derive(Serialize)]
struct MinutePoint {
    /// Unix timestamp (seconds) at the start of the minute.
    t: i64,
    requests: u64,
    errors: u64,
}

#[derive(Serialize)]
struct CertInfo {
    expires_at: i64,
    days_left: i64,
}

#[derive(Serialize)]
struct Snapshot {
    version: &'static str,
    now: i64,
    started_at: i64,
    uptime_secs: u64,
    requests_total: u64,
    status_classes: [u64; 5],
    in_flight: i64,
    last_minute_requests: usize,
    p50_ms: Option<f64>,
    p95_ms: Option<f64>,
    p99_ms: Option<f64>,
    memory_rss_bytes: Option<u64>,
    load_avg: Option<[f64; 3]>,
    cpus: usize,
    cert: Option<CertInfo>,
    /// One point per minute for the last `HISTORY_MINUTES`, zero-filled.
    history: Vec<MinutePoint>,
}

fn percentile(sorted: &[u32], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    Some(sorted[idx] as f64 / 1000.0)
}

/// Resident memory of this process, from /proc (Linux only).
fn memory_rss() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

fn load_avg() -> Option<[f64; 3]> {
    let s = std::fs::read_to_string("/proc/loadavg").ok()?;
    let mut it = s.split_whitespace().map(|v| v.parse::<f64>().ok());
    Some([it.next()??, it.next()??, it.next()??])
}

async fn cert_info(state: &AppState) -> Option<CertInfo> {
    if !state.config.ssl_enabled {
        return None;
    }
    let pem = tokio::fs::read(state.config.ssl_cert_path.as_ref()?).await.ok()?;
    let expires_at = crate::acme::parse_not_after(&pem)?;
    Some(CertInfo { expires_at, days_left: (expires_at - Utc::now().timestamp()) / 86_400 })
}

async fn snapshot(state: &AppState) -> Snapshot {
    let m = &*METRICS;
    let now_unix = Utc::now().timestamp();
    let minute = now_unix / 60;
    let (mut latencies, minutes) = {
        let mut inner = m.inner.lock().unwrap();
        prune(&mut inner, Instant::now(), minute);
        let lat: Vec<u32> = inner.samples.iter().map(|(_, us)| *us).collect();
        (lat, inner.minutes.clone())
    };
    latencies.sort_unstable();

    let mut history = Vec::with_capacity(HISTORY_MINUTES as usize);
    let mut recorded = minutes.iter().peekable();
    for mm in (minute - HISTORY_MINUTES + 1)..=minute {
        let (requests, errors) = match recorded.peek() {
            Some((rm, r, e)) if *rm == mm => {
                let v = (*r, *e);
                recorded.next();
                v
            }
            _ => (0, 0),
        };
        history.push(MinutePoint { t: mm * 60, requests, errors });
    }

    Snapshot {
        version: env!("CARGO_PKG_VERSION"),
        now: now_unix,
        started_at: m.started_at,
        uptime_secs: m.started.elapsed().as_secs(),
        requests_total: m.total.load(Ordering::Relaxed),
        status_classes: std::array::from_fn(|i| m.classes[i].load(Ordering::Relaxed)),
        in_flight: m.in_flight.load(Ordering::Relaxed),
        last_minute_requests: latencies.len(),
        p50_ms: percentile(&latencies, 0.50),
        p95_ms: percentile(&latencies, 0.95),
        p99_ms: percentile(&latencies, 0.99),
        memory_rss_bytes: memory_rss(),
        load_avg: load_avg(),
        cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        cert: cert_info(state).await,
        history,
    }
}

async fn status_json(State(state): State<Arc<AppState>>) -> Response {
    let mut resp = Json(snapshot(&state).await).into_response();
    resp.extensions_mut().insert(Untracked);
    resp.headers_mut().insert(axum::http::header::CACHE_CONTROL, "no-store".parse().unwrap());
    resp
}

async fn status_page(
    State(state): State<Arc<AppState>>,
    Extension(template): Extension<Arc<String>>,
) -> Response {
    let mut ctx = tera::Context::new();
    ctx.insert("domain", &state.config.domain);
    match state.tera.render(template.trim_start_matches("static/"), &ctx) {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(e) => {
            eprintln!("status page render error: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
        }
    }
}

/// Routes for a status mount: the page at the mount root, the snapshot at `/json`.
pub fn router(template_path: &str) -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(status_page))
        .route("/json", get(status_json))
        .layer(Extension(Arc::new(template_path.to_string())))
}
