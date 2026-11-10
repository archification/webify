// The Lab: small public developer tools that show the server working live.
// Mounted with `"/lab" = ["static/lab.html", "lab"]` in config.toml:
//
//   {mount}/                 index page (the configured template)
//   {mount}/inspect          request inspector (HTML, or JSON for non-browsers)
//   {mount}/hooks            webhook catcher: create a bin, watch requests arrive
//   {mount}/hook/{id}/...    the capture endpoint, any method
//   {mount}/paste, /p/{id}   expiring pastebin
//   {mount}/board            shared live canvas (see board.rs)
//
// Everything is anonymous, so everything is capped: per-IP rate limits, size
// limits, expiry, and pages carry noindex so user content isn't search-indexed.

use axum::{
    Extension, Form, Json, Router,
    body::Bytes,
    extract::{ConnectInfo, FromRequestParts, DefaultBodyLimit, OriginalUri, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header, request::Parts},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{any, get},
};
use chrono::Utc;
use rand::{Rng, distr::Alphanumeric};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::AppState;
use crate::forum::ForumDb;

// ─── Limits ──────────────────────────────────────────────────────────────────

const BIN_TTL: Duration = Duration::from_secs(24 * 3600);
const MAX_BINS: usize = 1000;
const MAX_EVENTS_PER_BIN: usize = 50;
/// Request bodies above this are rejected outright; up to `MAX_STORED_BODY` is kept.
const MAX_HOOK_BODY: usize = 256 * 1024;
const MAX_STORED_BODY: usize = 64 * 1024;
const BINS_PER_IP_PER_HOUR: usize = 20;
const HITS_PER_BIN_PER_MINUTE: usize = 120;

const MAX_PASTE: usize = 64 * 1024;
const MAX_LIVE_PASTES: i64 = 5000;
const PASTES_PER_IP_PER_HOUR: usize = 15;
/// Allowed paste lifetimes: (form value, seconds, label).
const PASTE_EXPIRY: &[(&str, i64, &str)] = &[
    ("10m", 600, "10 minutes"),
    ("1h", 3600, "1 hour"),
    ("1d", 86_400, "1 day"),
    ("7d", 604_800, "7 days"),
];
/// Highlighting choices: (stored value, label). Values are highlight.js names.
const PASTE_LANGS: &[(&str, &str)] = &[
    ("auto", "Detect automatically"),
    ("plaintext", "Plain text"),
    ("bash", "Bash / shell"),
    ("c", "C"),
    ("cpp", "C++"),
    ("csharp", "C#"),
    ("css", "CSS"),
    ("diff", "Diff"),
    ("go", "Go"),
    ("xml", "HTML / XML"),
    ("java", "Java"),
    ("javascript", "JavaScript"),
    ("json", "JSON"),
    ("kotlin", "Kotlin"),
    ("lua", "Lua"),
    ("markdown", "Markdown"),
    ("php", "PHP"),
    ("python", "Python"),
    ("ruby", "Ruby"),
    ("rust", "Rust"),
    ("sql", "SQL"),
    ("swift", "Swift"),
    ("ini", "TOML / INI"),
    ("typescript", "TypeScript"),
    ("yaml", "YAML"),
];

// ─── Shared helpers ──────────────────────────────────────────────────────────

/// Sliding-window limiter: at most `max` events per key within `window`.
pub struct RateLimiter {
    window: Duration,
    max: usize,
    hits: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        Self { window, max, hits: Mutex::new(HashMap::new()) }
    }

    /// Records a hit and returns whether it's within the limit.
    pub fn allow(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut hits = self.hits.lock().unwrap();
        // Keep the map from growing without bound on a long-running server.
        if hits.len() > 10_000 {
            let window = self.window;
            hits.retain(|_, q| q.back().is_some_and(|t| now.duration_since(*t) < window));
        }
        let q = hits.entry(key.to_string()).or_default();
        while q.front().is_some_and(|t| now.duration_since(*t) >= self.window) {
            q.pop_front();
        }
        if q.len() >= self.max {
            return false;
        }
        q.push_back(now);
        true
    }
}

fn random_id(len: usize) -> String {
    rand::rng().sample_iter(&Alphanumeric).take(len).map(char::from).collect()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Where this router was nested (e.g. "/lab") and its index template.
#[derive(Clone)]
pub struct Mount {
    pub path: Arc<String>,
    template: Arc<String>,
}

/// The host the client asked for, for building absolute links. HTTP/2 requests
/// carry it in `:authority` (the URI) rather than a Host header.
pub struct ReqHost(String);

impl<S: Send + Sync> FromRequestParts<S> for ReqHost {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let host = parts
            .headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .map(str::to_string)
            .or_else(|| {
                parts.extensions.get::<OriginalUri>()
                    .and_then(|u| u.0.authority())
                    .or_else(|| parts.uri.authority())
                    .map(|a| a.as_str().to_string())
            })
            .unwrap_or_default();
        Ok(ReqHost(host))
    }
}

fn render(state: &AppState, template: &str, ctx: &tera::Context) -> Response {
    match state.tera.render(template, ctx) {
        Ok(html) => {
            let mut resp = Html(html).into_response();
            resp.headers_mut().insert("x-robots-tag", HeaderValue::from_static("noindex"));
            resp
        }
        Err(e) => {
            eprintln!("lab render error ({}): {}", template, e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
        }
    }
}

fn base_ctx(mount: &Mount, host: &ReqHost) -> tera::Context {
    let mut ctx = tera::Context::new();
    ctx.insert("lab_path", mount.path.as_str());
    ctx.insert("host", &host.0);
    ctx
}

fn too_many(msg: &str) -> Response {
    (StatusCode::TOO_MANY_REQUESTS, msg.to_string()).into_response()
}

// ─── Index ───────────────────────────────────────────────────────────────────

async fn index(State(state): State<Arc<AppState>>, Extension(m): Extension<Mount>, host: ReqHost) -> Response {
    let ctx = base_ctx(&m, &host);
    let mut resp = render(&state, m.template.trim_start_matches("static/"), &ctx);
    // The index itself is fine to index; only user content pages aren't.
    resp.headers_mut().remove("x-robots-tag");
    resp
}

// ─── Request inspector ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct Inspection {
    method: String,
    path: String,
    query: Option<String>,
    http_version: String,
    client_ip: String,
    headers: Vec<(String, String)>,
    cookie_names: Vec<String>,
}

fn version_str(v: axum::http::Version) -> &'static str {
    match v {
        axum::http::Version::HTTP_09 => "HTTP/0.9",
        axum::http::Version::HTTP_10 => "HTTP/1.0",
        axum::http::Version::HTTP_11 => "HTTP/1.1",
        axum::http::Version::HTTP_2 => "HTTP/2",
        axum::http::Version::HTTP_3 => "HTTP/3",
        _ => "unknown",
    }
}

/// Cookie values never leave the server (they may be session tokens); only names are shown.
fn cookie_names(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|p| p.split_once('=').map(|(k, _)| k.trim().to_string()))
        .filter(|k| !k.is_empty())
        .collect()
}

fn header_list(headers: &HeaderMap) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| {
            let value = if *k == header::COOKIE {
                "[hidden: values of this site's cookies are never shown]".to_string()
            } else {
                String::from_utf8_lossy(v.as_bytes()).into_owned()
            };
            (k.as_str().to_string(), value)
        })
        .collect();
    out.sort();
    out
}

async fn inspect(
    State(state): State<Arc<AppState>>,
    Extension(m): Extension<Mount>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    OriginalUri(uri): OriginalUri,
    host: ReqHost,
    req: Request,
) -> Response {
    let headers = req.headers();
    let data = Inspection {
        method: req.method().to_string(),
        // The full path as sent, not the one relative to the lab mount.
        path: uri.path().to_string(),
        query: uri.query().map(str::to_string),
        http_version: version_str(req.version()).to_string(),
        client_ip: addr.ip().to_canonical().to_string(),
        headers: header_list(headers),
        cookie_names: cookie_names(headers),
    };
    let wants_html = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|a| a.contains("text/html"))
        && !req.uri().query().is_some_and(|q| q.split('&').any(|p| p == "format=json"));
    if !wants_html {
        let mut resp = Json(&data).into_response();
        resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return resp;
    }
    let mut ctx = base_ctx(&m, &host);
    ctx.insert("r", &data);
    let mut resp = render(&state, "lab-inspect.html", &ctx);
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

// ─── Webhook catcher ─────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
struct HookEvent {
    seq: u64,
    /// Unix milliseconds.
    at: i64,
    method: String,
    path: String,
    query: Option<String>,
    headers: Vec<(String, String)>,
    body: String,
    body_size: usize,
    body_truncated: bool,
    /// True when the body wasn't valid UTF-8 (shown lossily).
    body_binary: bool,
}

struct Bin {
    created: Instant,
    created_at: i64,
    seq: u64,
    events: VecDeque<HookEvent>,
}

static BINS: LazyLock<Mutex<HashMap<String, Bin>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static BIN_CREATE_LIMIT: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(BINS_PER_IP_PER_HOUR, Duration::from_secs(3600)));
static BIN_HIT_LIMIT: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(HITS_PER_BIN_PER_MINUTE, Duration::from_secs(60)));

fn prune_bins(bins: &mut HashMap<String, Bin>) {
    bins.retain(|_, b| b.created.elapsed() < BIN_TTL);
}

async fn hooks_landing(State(state): State<Arc<AppState>>, Extension(m): Extension<Mount>, host: ReqHost) -> Response {
    let mut ctx = base_ctx(&m, &host);
    ctx.insert("bin_id", "");
    render(&state, "lab-hooks.html", &ctx)
}

async fn hooks_create(
    Extension(m): Extension<Mount>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if !BIN_CREATE_LIMIT.allow(&ip_key(addr.ip())) {
        return too_many("Too many webhook bins from your address. Try again in an hour.");
    }
    let id = random_id(16);
    {
        let mut bins = BINS.lock().unwrap();
        prune_bins(&mut bins);
        if bins.len() >= MAX_BINS {
            return (StatusCode::SERVICE_UNAVAILABLE, "The webhook catcher is full right now. Try again later.").into_response();
        }
        bins.insert(id.clone(), Bin {
            created: Instant::now(),
            created_at: Utc::now().timestamp(),
            seq: 0,
            events: VecDeque::new(),
        });
    }
    Redirect::to(&format!("{}/hooks/{}", m.path, id)).into_response()
}

async fn hooks_view(
    State(state): State<Arc<AppState>>,
    Extension(m): Extension<Mount>,
    host: ReqHost,
    Path(id): Path<String>,
) -> Response {
    let expires_at = {
        let mut bins = BINS.lock().unwrap();
        prune_bins(&mut bins);
        match bins.get(&id) {
            Some(b) => b.created_at + BIN_TTL.as_secs() as i64,
            None => {
                let mut ctx = base_ctx(&m, &host);
                ctx.insert("bin_id", "");
                ctx.insert("missing", &true);
                let mut resp = render(&state, "lab-hooks.html", &ctx);
                *resp.status_mut() = StatusCode::NOT_FOUND;
                return resp;
            }
        }
    };
    let mut ctx = base_ctx(&m, &host);
    ctx.insert("bin_id", &id);
    ctx.insert("expires_at", &expires_at);
    render(&state, "lab-hooks.html", &ctx)
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    after: u64,
}

async fn hooks_events(Path(id): Path<String>, Query(q): Query<EventsQuery>) -> Response {
    let bins = BINS.lock().unwrap();
    let Some(bin) = bins.get(&id).filter(|b| b.created.elapsed() < BIN_TTL) else {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "bin not found or expired" }))).into_response();
    };
    let events: Vec<&HookEvent> = bin.events.iter().filter(|e| e.seq > q.after).collect();
    let mut resp = Json(json!({ "events": events, "total": bin.seq })).into_response();
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

async fn hook_capture(
    Path(params): Path<HashMap<String, String>>,
    req: Request,
) -> Response {
    let id = params.get("id").cloned().unwrap_or_default();
    if !valid_id(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !BINS.lock().unwrap().get(&id).is_some_and(|b| b.created.elapsed() < BIN_TTL) {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "bin not found or expired" }))).into_response();
    }
    if !BIN_HIT_LIMIT.allow(&id) {
        return too_many("This bin is receiving too many requests; slow down.");
    }

    let (parts, body) = req.into_parts();
    let bytes: Bytes = match axum::body::to_bytes(body, MAX_HOOK_BODY).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "Body too large (256 KB max)").into_response(),
    };
    let body_binary = std::str::from_utf8(&bytes).is_err();
    let kept = &bytes[..bytes.len().min(MAX_STORED_BODY)];

    let mut bins = BINS.lock().unwrap();
    let Some(bin) = bins.get_mut(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    bin.seq += 1;
    let event = HookEvent {
        seq: bin.seq,
        at: Utc::now().timestamp_millis(),
        method: parts.method.to_string(),
        path: params.get("rest").map(|r| format!("/{}", r)).unwrap_or_else(|| "/".to_string()),
        query: parts.uri.query().map(str::to_string),
        headers: header_list(&parts.headers),
        body: String::from_utf8_lossy(kept).into_owned(),
        body_size: bytes.len(),
        body_truncated: bytes.len() > MAX_STORED_BODY,
        body_binary,
    };
    let seq = event.seq;
    if bin.events.len() >= MAX_EVENTS_PER_BIN {
        bin.events.pop_front();
    }
    bin.events.push_back(event);
    drop(bins);

    let mut resp = Json(json!({ "ok": true, "seq": seq })).into_response();
    let h = resp.headers_mut();
    // Let browser-based senders (fetch from any page) hit the endpoint too.
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    if parts.method == Method::OPTIONS {
        h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("*"));
        h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("*"));
    }
    resp
}

// ─── Pastebin ────────────────────────────────────────────────────────────────

static PASTE_LIMIT: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(PASTES_PER_IP_PER_HOUR, Duration::from_secs(3600)));

pub async fn init_tables(db: &ForumDb) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS lab_pastes (
            id TEXT PRIMARY KEY,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL
        )"
    ).execute(&**db).await.unwrap();
    // Added after the first release; fails harmlessly once the column exists.
    let _ = sqlx::query("ALTER TABLE lab_pastes ADD COLUMN lang TEXT NOT NULL DEFAULT 'auto'")
        .execute(&**db)
        .await;
}

async fn delete_expired_pastes(db: &ForumDb) {
    let _ = sqlx::query("DELETE FROM lab_pastes WHERE expires_at <= ?")
        .bind(Utc::now().timestamp())
        .execute(&**db)
        .await;
}

fn expiry_options() -> Vec<serde_json::Value> {
    PASTE_EXPIRY.iter().map(|(v, _, l)| json!({ "value": v, "label": l })).collect()
}

fn lang_options() -> Vec<serde_json::Value> {
    PASTE_LANGS.iter().map(|(v, l)| json!({ "value": v, "label": l })).collect()
}

/// Maps a language name or common alias (as written after a ``` fence) to a
/// PASTE_LANGS value.
fn normalize_lang(name: &str) -> Option<&'static str> {
    let name = name.trim().to_ascii_lowercase();
    let canonical = match name.as_str() {
        "rs" => "rust",
        "js" | "jsx" | "mjs" | "node" => "javascript",
        "ts" | "tsx" => "typescript",
        "py" | "python3" => "python",
        "sh" | "shell" | "zsh" | "console" => "bash",
        "yml" => "yaml",
        "html" | "htm" | "svg" => "xml",
        "toml" => "ini",
        "c++" | "cc" | "hpp" => "cpp",
        "cs" | "c#" => "csharp",
        "golang" => "go",
        "md" => "markdown",
        "kt" => "kotlin",
        "rb" => "ruby",
        "patch" => "diff",
        "text" | "txt" | "plain" => "plaintext",
        other => other,
    };
    PASTE_LANGS.iter().map(|(v, _)| *v).find(|v| *v == canonical)
}

/// If the whole paste is one markdown code block, returns the fence's
/// language (if any) and the code inside it.
fn strip_fence(content: &str) -> Option<(Option<&str>, &str)> {
    let rest = content.trim().strip_prefix("```")?;
    let (info, body) = rest.split_once('\n')?;
    let body = body.strip_suffix("```")?.trim_end_matches('\n');
    if body.lines().any(|l| l.trim_start().starts_with("```")) {
        return None;
    }
    let lang = info.split_whitespace().next();
    Some((lang, body))
}

async fn paste_form(State(state): State<Arc<AppState>>, Extension(m): Extension<Mount>, host: ReqHost) -> Response {
    let mut ctx = base_ctx(&m, &host);
    ctx.insert("expiry_options", &expiry_options());
    ctx.insert("lang_options", &lang_options());
    ctx.insert("max_kb", &(MAX_PASTE / 1024));
    render(&state, "lab-paste.html", &ctx)
}

#[derive(Deserialize)]
struct PasteForm {
    content: String,
    expiry: String,
    #[serde(default)]
    lang: String,
}

async fn paste_create(
    State(state): State<Arc<AppState>>,
    Extension(m): Extension<Mount>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    host: ReqHost,
    Form(form): Form<PasteForm>,
) -> Response {
    let fail = |msg: &str, form: &PasteForm| {
        let mut ctx = base_ctx(&m, &host);
        ctx.insert("expiry_options", &expiry_options());
        ctx.insert("lang_options", &lang_options());
        ctx.insert("max_kb", &(MAX_PASTE / 1024));
        ctx.insert("error", msg);
        ctx.insert("content", &form.content);
        ctx.insert("expiry", &form.expiry);
        ctx.insert("lang", &form.lang);
        let mut resp = render(&state, "lab-paste.html", &ctx);
        *resp.status_mut() = StatusCode::BAD_REQUEST;
        resp
    };
    if form.content.trim().is_empty() {
        return fail("The paste is empty.", &form);
    }
    if form.content.len() > MAX_PASTE {
        return fail(&format!("Pastes are limited to {} KB.", MAX_PASTE / 1024), &form);
    }
    let Some((_, ttl, _)) = PASTE_EXPIRY.iter().find(|(v, _, _)| *v == form.expiry) else {
        return fail("Pick an expiry time.", &form);
    };
    if !PASTE_LIMIT.allow(&ip_key(addr.ip())) {
        return fail("Too many pastes from your address. Try again in an hour.", &form);
    }
    delete_expired_pastes(&state.forum_db).await;
    let live: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lab_pastes")
        .fetch_one(&*state.forum_db)
        .await
        .unwrap_or(0);
    if live >= MAX_LIVE_PASTES {
        return fail("The pastebin is full right now. Try again later.", &form);
    }

    // Browsers submit textareas with CRLF line endings.
    let mut content = form.content.replace("\r\n", "\n");
    let mut lang = normalize_lang(&form.lang).unwrap_or("auto");
    // A paste that's one fenced block, as copied from chat or markdown, is
    // stored as just the code, highlighted as the fence's language.
    if let Some((fence_lang, body)) = strip_fence(&content) {
        if lang == "auto" {
            lang = fence_lang.and_then(normalize_lang).unwrap_or("auto");
        }
        content = body.to_string();
    }

    let id = random_id(10);
    let now = Utc::now().timestamp();
    let inserted = sqlx::query("INSERT INTO lab_pastes (id, content, lang, created_at, expires_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(&content)
        .bind(lang)
        .bind(now)
        .bind(now + ttl)
        .execute(&*state.forum_db)
        .await;
    if inserted.is_err() {
        return fail("Couldn't save the paste. Try again.", &form);
    }
    Redirect::to(&format!("{}/p/{}", m.path, id)).into_response()
}

async fn load_paste(db: &ForumDb, id: &str) -> Option<(String, String, i64, i64)> {
    if !valid_id(id) {
        return None;
    }
    sqlx::query_as("SELECT content, lang, created_at, expires_at FROM lab_pastes WHERE id = ? AND expires_at > ?")
        .bind(id)
        .bind(Utc::now().timestamp())
        .fetch_optional(&**db)
        .await
        .unwrap_or(None)
}

async fn paste_view(
    State(state): State<Arc<AppState>>,
    Extension(m): Extension<Mount>,
    host: ReqHost,
    Path(id): Path<String>,
) -> Response {
    let mut ctx = base_ctx(&m, &host);
    ctx.insert("expiry_options", &expiry_options());
    ctx.insert("lang_options", &lang_options());
    ctx.insert("max_kb", &(MAX_PASTE / 1024));
    match load_paste(&state.forum_db, &id).await {
        Some((content, lang, created_at, expires_at)) => {
            let lang_label = PASTE_LANGS.iter().find(|(v, _)| *v == lang).map(|(_, l)| *l);
            ctx.insert("paste_id", &id);
            ctx.insert("paste", &content);
            ctx.insert("paste_lang", &lang);
            ctx.insert("paste_lang_label", &lang_label.unwrap_or(""));
            // Auto-detection only considers the languages offered in the form.
            let auto_langs: Vec<&str> = PASTE_LANGS.iter().map(|(v, _)| *v)
                .filter(|v| *v != "auto" && *v != "plaintext").collect();
            ctx.insert("auto_langs", &serde_json::to_string(&auto_langs).unwrap());
            ctx.insert("created_at", &created_at);
            ctx.insert("expires_at", &expires_at);
            ctx.insert("lines", &content.lines().count());
            ctx.insert("bytes", &content.len());
            render(&state, "lab-paste.html", &ctx)
        }
        None => {
            ctx.insert("missing", &true);
            let mut resp = render(&state, "lab-paste.html", &ctx);
            *resp.status_mut() = StatusCode::NOT_FOUND;
            resp
        }
    }
}

async fn paste_raw(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let Some((content, _, _, _)) = load_paste(&state.forum_db, &id).await else {
        return (StatusCode::NOT_FOUND, "Paste not found or expired").into_response();
    };
    let mut resp = content.into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    // Raw pastes are untrusted: never let a browser treat them as anything but text.
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; sandbox"));
    h.insert("x-robots-tag", HeaderValue::from_static("noindex"));
    resp
}

// ─── Router ──────────────────────────────────────────────────────────────────

/// Routes for a lab mount. Also starts the background cleanup task.
pub fn router(mount_path: &str, template_path: &str, db: ForumDb) -> Router<Arc<AppState>> {
    let mount = Mount {
        path: Arc::new(mount_path.trim_end_matches('/').to_string()),
        template: Arc::new(template_path.to_string()),
    };
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(600));
        loop {
            tick.tick().await;
            prune_bins(&mut BINS.lock().unwrap());
            delete_expired_pastes(&db).await;
        }
    });
    Router::new()
        .route("/", get(index))
        .route("/inspect", get(inspect))
        .route("/hooks", get(hooks_landing).post(hooks_create))
        .route("/hooks/{id}", get(hooks_view))
        .route("/hooks/{id}/events", get(hooks_events))
        .route("/hook/{id}", any(hook_capture).layer(DefaultBodyLimit::max(MAX_HOOK_BODY)))
        .route("/hook/{id}/{*rest}", any(hook_capture).layer(DefaultBodyLimit::max(MAX_HOOK_BODY)))
        .route("/paste", get(paste_form).post(paste_create).layer(DefaultBodyLimit::max(MAX_PASTE * 4)))
        .route("/p/{id}", get(paste_view))
        .route("/p/{id}/raw", get(paste_raw))
        .route("/board", get(crate::board::page))
        .route("/board/ws", get(crate::board::ws))
        .layer(Extension(mount))
}

/// Client IP helper shared with board.rs.
pub fn ip_key(ip: IpAddr) -> String {
    ip.to_canonical().to_string()
}
