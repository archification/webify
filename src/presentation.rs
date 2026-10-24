// Interactive presentations: branching, media-driven slide decks built in the
// browser (visual builder or plain-text script) and played back by
// static/present-engine.js. Invite-only — accounts are created from the admin
// dashboard, there is deliberately no self-registration route. Mounted with a
// `["unused", "presentation"]` route in config.toml.

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Form, Multipart, Path as AxPath, Query, Request, State},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use crate::AppState;
use crate::forum::ForumDb;
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "present_session";
const MEDIA_ROOT: &str = "presentation_media";
const SESSION_DAYS: i64 = 30;
const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MIN_PASSWORD_LEN: usize = 8;

const IMAGE_EXTS: &[&str] = &["webp", "png", "jpg", "jpeg", "gif", "avif", "bmp"];
const VIDEO_EXTS: &[&str] = &["mp4", "webm", "m4v", "ogv", "mov"];
const AUDIO_EXTS: &[&str] = &["mp3", "ogg", "oga", "wav", "m4a", "aac", "flac", "opus", "weba"];

/// URL prefix the presentation routes are nested under (e.g. "/present"),
/// handed to every handler so redirects and templates don't hardcode it.
#[derive(Clone)]
pub struct PresentBase(pub String);

pub async fn init_tables(db: &ForumDb) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS presentation_accounts (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE COLLATE NOCASE,
            display_name TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_by TEXT NOT NULL,
            created_at TEXT NOT NULL,
            last_login_at TEXT
        )"
    ).execute(&**db).await.unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS presentation_sessions (
            token TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL
        )"
    ).execute(&**db).await.unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS presentations (
            id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            visibility TEXT NOT NULL DEFAULT 'private',
            document TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )"
    ).execute(&**db).await.unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS presentation_media (
            presentation_id TEXT NOT NULL,
            filename TEXT NOT NULL,
            kind TEXT NOT NULL,
            size INTEGER NOT NULL,
            uploaded_at TEXT NOT NULL,
            PRIMARY KEY (presentation_id, filename)
        )"
    ).execute(&**db).await.unwrap();
}

pub fn router(base: &str, upload_limit: usize) -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(index))
        .route("/login", get(login_form).post(login))
        .route("/logout", get(logout))
        .route("/password", post(change_password))
        .route("/new", post(create_presentation))
        .route("/edit/{id}", get(edit_page))
        .route("/play/{id}", get(play_page))
        .route("/delete/{id}", post(delete_presentation))
        .route("/api/{id}", get(api_get).post(api_save)
            .layer(DefaultBodyLimit::max(MAX_DOCUMENT_BYTES)))
        .route("/media/{id}", post(upload_media)
            .layer(DefaultBodyLimit::max(upload_limit)))
        .route("/media/{id}/delete", post(delete_media))
        .route("/media/{id}/{filename}", get(serve_media))
        .layer(Extension(PresentBase(base.trim_end_matches('/').to_string())))
}

/// The first configured presentation mount point, for linking to it from the
/// admin dashboard.
pub fn configured_path(config: &crate::config::Config) -> Option<String> {
    config.sites.values().flatten().find_map(|(path, settings)| {
        (settings.get(1).map(String::as_str) == Some("presentation"))
            .then(|| path.trim_end_matches('/').to_string())
    })
}

// ─── Models ──────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow, Clone)]
pub struct Account {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub status: String,
}

#[derive(sqlx::FromRow)]
struct PresentationRow {
    id: String,
    owner_id: String,
    title: String,
    description: String,
    visibility: String,
    document: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct ListingRow {
    id: String,
    owner_id: String,
    title: String,
    description: String,
    visibility: String,
    document: String,
    updated_at: String,
    author: String,
}

#[derive(Serialize)]
struct Card {
    id: String,
    title: String,
    description: String,
    visibility: String,
    updated_at: String,
    author: String,
    page_count: usize,
    cover: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
struct MediaItem {
    filename: String,
    kind: String,
    size: i64,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct AdminAccountRow {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub status: String,
    pub created_by: String,
    pub created_at: String,
    pub last_login_at: Option<String>,
    pub presentation_count: i64,
    pub shared_count: i64,
    pub media_bytes: i64,
}

impl PresentationRow {
    fn can_view(&self, account: &Account) -> bool {
        self.owner_id == account.id || self.visibility == "shared"
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn render(state: &AppState, template: &str, ctx: &tera::Context) -> Response {
    match state.tera.render(template, ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error rendering {}: {}", template, e),
        )
            .into_response(),
    }
}

/// Serialize for embedding inside a `<script type="application/json">` tag.
/// Tera autoescaping is disabled site-wide, so `<` must be neutralized here or
/// a title containing `</script>` would break out of the tag.
fn json_for_script(value: &Value) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".to_string())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "ok": false, "error": message }))).into_response()
}

fn valid_id(id: &str) -> bool {
    Uuid::parse_str(id).is_ok()
}

fn valid_username(username: &str) -> bool {
    (3..=32).contains(&username.len())
        && username.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

pub fn media_kind(filename: &str) -> Option<&'static str> {
    let ext = filename.rsplit_once('.')?.1.to_ascii_lowercase();
    if IMAGE_EXTS.contains(&ext.as_str()) {
        Some("image")
    } else if VIDEO_EXTS.contains(&ext.as_str()) {
        Some("video")
    } else if AUDIO_EXTS.contains(&ext.as_str()) {
        Some("audio")
    } else {
        None
    }
}

/// Normalize an uploaded filename to `[A-Za-z0-9_-]+.ext` so it can be typed
/// into a script verbatim and is always safe as a path component. Returns None
/// for file types the player can't show.
fn clean_filename(original: &str) -> Option<String> {
    let base = original.rsplit(['/', '\\']).next().unwrap_or(original);
    let (stem, ext) = base.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    media_kind(&format!("x.{ext}"))?;
    let mut clean = String::new();
    for c in stem.chars() {
        let c = if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' };
        if !(c == '_' && clean.ends_with('_')) {
            clean.push(c);
        }
    }
    let clean: String = clean.trim_matches('_').chars().take(80).collect();
    let clean = if clean.is_empty() { "file".to_string() } else { clean };
    Some(format!("{clean}.{ext}"))
}

fn media_dir(presentation_id: &str) -> PathBuf {
    PathBuf::from(MEDIA_ROOT).join(presentation_id)
}

fn session_cookie(token: String, base: &str, secure: bool) -> Cookie<'static> {
    // Built from a string because axum-extra doesn't re-export the `time`
    // crate that `CookieBuilder::max_age` wants.
    let max_age = SESSION_DAYS * 24 * 60 * 60;
    let secure = if secure { "; Secure" } else { "" };
    let raw = format!("{SESSION_COOKIE}={token}; Path={base}; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}");
    Cookie::parse(raw)
        .map(Cookie::into_owned)
        .unwrap_or_else(|_| Cookie::new(SESSION_COOKIE, String::new()))
}

/// Only allow redirecting back into the presentation area after login.
fn safe_next(next: Option<&str>, base: &str) -> String {
    match next {
        Some(n) if (n == base || n.starts_with(&format!("{base}/")) || n.starts_with(&format!("{base}?")))
            && !n.contains("://") => n.to_string(),
        _ => base.to_string(),
    }
}

async fn current_account(state: &AppState, jar: &CookieJar) -> Option<Account> {
    let token = jar.get(SESSION_COOKIE)?.value().to_string();
    let (account_id, expires): (String, String) = sqlx::query_as(
        "SELECT account_id, expires_at FROM presentation_sessions WHERE token = ?",
    )
    .bind(&token)
    .fetch_optional(&*state.forum_db)
    .await
    .ok()??;
    let expires = chrono::DateTime::parse_from_rfc3339(&expires).ok()?;
    if expires < Utc::now() {
        let _ = sqlx::query("DELETE FROM presentation_sessions WHERE token = ?")
            .bind(&token)
            .execute(&*state.forum_db)
            .await;
        return None;
    }
    sqlx::query_as::<_, Account>(
        "SELECT id, username, display_name, password_hash, status FROM presentation_accounts \
         WHERE id = ? AND status = 'active'",
    )
    .bind(&account_id)
    .fetch_optional(&*state.forum_db)
    .await
    .ok()
    .flatten()
}

/// Page handlers: the signed-in account, or a redirect to the login page.
async fn require_account(
    state: &AppState,
    jar: &CookieJar,
    base: &str,
    next: &str,
) -> Result<Account, Response> {
    match current_account(state, jar).await {
        Some(a) => Ok(a),
        None => Err(Redirect::to(&format!(
            "{base}/login?next={}",
            urlencoding::encode(next)
        ))
        .into_response()),
    }
}

async fn load_presentation(state: &AppState, id: &str) -> Option<PresentationRow> {
    if !valid_id(id) {
        return None;
    }
    sqlx::query_as::<_, PresentationRow>(
        "SELECT id, owner_id, title, description, visibility, document, updated_at \
         FROM presentations WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&*state.forum_db)
    .await
    .ok()
    .flatten()
}

async fn load_media(state: &AppState, presentation_id: &str) -> Vec<MediaItem> {
    sqlx::query_as::<_, MediaItem>(
        "SELECT filename, kind, size FROM presentation_media \
         WHERE presentation_id = ? ORDER BY filename",
    )
    .bind(presentation_id)
    .fetch_all(&*state.forum_db)
    .await
    .unwrap_or_default()
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

fn default_document() -> Value {
    json!({
        "format": 1,
        "start": "start",
        "vars": {},
        "pages": [{
            "id": "start",
            "text": "Welcome! Edit this page to get started.",
            "buttons": [],
        }],
    })
}

/// Light structural check. The builder validates in depth (targets,
/// expressions, media); the server only guarantees the player gets a
/// document shaped like one.
fn check_document(doc: &Value) -> Result<(), String> {
    let pages = doc
        .get("pages")
        .and_then(Value::as_array)
        .ok_or("Document has no pages list")?;
    if pages.is_empty() {
        return Err("A presentation needs at least one page".into());
    }
    let mut seen = std::collections::HashSet::new();
    for page in pages {
        let id = page.get("id").and_then(Value::as_str).ok_or("Every page needs an id")?;
        let id_ok = !id.is_empty()
            && id.len() <= 64
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !id_ok {
            return Err(format!("Invalid page id: {id}"));
        }
        if !seen.insert(id.to_string()) {
            return Err(format!("Duplicate page id: {id}"));
        }
    }
    Ok(())
}

fn to_card(row: ListingRow) -> Card {
    let doc: Value = serde_json::from_str(&row.document).unwrap_or(Value::Null);
    let pages = doc.get("pages").and_then(Value::as_array);
    let cover = pages.and_then(|pages| {
        pages.iter().find_map(|p| {
            let media = p.get("media").and_then(Value::as_str)?;
            (media_kind(media) == Some("image"))
                .then(|| format!("{}/{}", row.id, media))
        })
    });
    Card {
        page_count: pages.map(Vec::len).unwrap_or(0),
        cover,
        id: row.id,
        title: row.title,
        description: row.description,
        visibility: row.visibility,
        updated_at: row.updated_at,
        author: row.author,
    }
}

// ─── Auth handlers ───────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct LoginQuery {
    next: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
    next: Option<String>,
}

pub async fn login_form(
    State(state): State<Arc<AppState>>,
    Extension(PresentBase(base)): Extension<PresentBase>,
    jar: CookieJar,
    Query(q): Query<LoginQuery>,
) -> Response {
    let next = safe_next(q.next.as_deref(), &base);
    if current_account(&state, &jar).await.is_some() {
        return Redirect::to(&next).into_response();
    }
    let mut ctx = tera::Context::new();
    ctx.insert("base", &base);
    ctx.insert("next", &next);
    render(&state, "present-login.html", &ctx)
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Extension(PresentBase(base)): Extension<PresentBase>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> Response {
    let next = safe_next(form.next.as_deref(), &base);
    let account: Option<Account> = sqlx::query_as(
        "SELECT id, username, display_name, password_hash, status FROM presentation_accounts WHERE username = ?",
    )
    .bind(form.username.trim())
    .fetch_optional(&*state.forum_db)
    .await
    .ok()
    .flatten();

    let ok = account.as_ref().is_some_and(|a| {
        a.status == "active" && verify(&form.password, &a.password_hash).unwrap_or(false)
    });
    let Some(account) = account.filter(|_| ok) else {
        let mut ctx = tera::Context::new();
        ctx.insert("base", &base);
        ctx.insert("next", &next);
        ctx.insert("username", form.username.trim());
        ctx.insert("error", "Invalid username or password.");
        return render(&state, "present-login.html", &ctx);
    };

    let token = Uuid::new_v4().to_string();
    let now = Utc::now();
    let _ = sqlx::query(
        "INSERT INTO presentation_sessions (token, account_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&token)
    .bind(&account.id)
    .bind(now.to_rfc3339())
    .bind((now + Duration::days(SESSION_DAYS)).to_rfc3339())
    .execute(&*state.forum_db)
    .await;
    let _ = sqlx::query("UPDATE presentation_accounts SET last_login_at = ? WHERE id = ?")
        .bind(now.to_rfc3339())
        .bind(&account.id)
        .execute(&*state.forum_db)
        .await;

    let cookie = session_cookie(token, &base, state.config.ssl_enabled);
    (jar.add(cookie), Redirect::to(&next)).into_response()
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    Extension(PresentBase(base)): Extension<PresentBase>,
    jar: CookieJar,
) -> Response {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        let _ = sqlx::query("DELETE FROM presentation_sessions WHERE token = ?")
            .bind(c.value())
            .execute(&*state.forum_db)
            .await;
    }
    let cookie = Cookie::build(SESSION_COOKIE).path(base.clone()).build();
    (jar.remove(cookie), Redirect::to(&format!("{base}/login"))).into_response()
}

#[derive(Deserialize)]
pub struct ChangePasswordForm {
    current_password: String,
    new_password: String,
    confirm_password: String,
}

pub async fn change_password(
    State(state): State<Arc<AppState>>,
    Extension(PresentBase(base)): Extension<PresentBase>,
    jar: CookieJar,
    Form(form): Form<ChangePasswordForm>,
) -> Response {
    let account = match require_account(&state, &jar, &base, &base).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let fail = |msg: &str| Redirect::to(&format!("{base}?error={}", urlencoding::encode(msg))).into_response();
    if !verify(&form.current_password, &account.password_hash).unwrap_or(false) {
        return fail("Current password is incorrect.");
    }
    if form.new_password.len() < MIN_PASSWORD_LEN {
        return fail("New password must be at least 8 characters.");
    }
    if form.new_password != form.confirm_password {
        return fail("New passwords do not match.");
    }
    let Ok(hashed) = hash(&form.new_password, DEFAULT_COST) else {
        return fail("Could not update password.");
    };
    let _ = sqlx::query("UPDATE presentation_accounts SET password_hash = ? WHERE id = ?")
        .bind(&hashed)
        .bind(&account.id)
        .execute(&*state.forum_db)
        .await;
    // Sign out every other device that was using the old password.
    let current = jar.get(SESSION_COOKIE).map(|c| c.value().to_string()).unwrap_or_default();
    let _ = sqlx::query("DELETE FROM presentation_sessions WHERE account_id = ? AND token != ?")
        .bind(&account.id)
        .bind(&current)
        .execute(&*state.forum_db)
        .await;
    Redirect::to(&format!("{base}?notice=Password+updated")).into_response()
}

// ─── Library / builder / player ──────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct IndexQuery {
    notice: Option<String>,
    error: Option<String>,
}

pub async fn index(
    State(state): State<Arc<AppState>>,
    Extension(PresentBase(base)): Extension<PresentBase>,
    jar: CookieJar,
    Query(q): Query<IndexQuery>,
) -> Response {
    let account = match require_account(&state, &jar, &base, &base).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let rows: Vec<ListingRow> = sqlx::query_as(
        "SELECT p.id, p.owner_id, p.title, p.description, p.visibility, p.document, p.updated_at, \
                a.display_name AS author \
         FROM presentations p JOIN presentation_accounts a ON a.id = p.owner_id \
         WHERE p.owner_id = ? OR p.visibility = 'shared' \
         ORDER BY p.updated_at DESC",
    )
    .bind(&account.id)
    .fetch_all(&*state.forum_db)
    .await
    .unwrap_or_default();

    let (mine, shared): (Vec<_>, Vec<_>) = rows.into_iter().partition(|r| r.owner_id == account.id);
    let data = json!({
        "base": base,
        "account": { "username": account.username, "display_name": account.display_name },
        "mine": mine.into_iter().map(to_card).collect::<Vec<_>>(),
        "shared": shared.into_iter().map(to_card).collect::<Vec<_>>(),
        "notice": q.notice.unwrap_or_default(),
        "error": q.error.unwrap_or_default(),
    });
    let mut ctx = tera::Context::new();
    ctx.insert("base", &base);
    ctx.insert("data_json", &json_for_script(&data));
    render(&state, "present-index.html", &ctx)
}

#[derive(Deserialize)]
pub struct CreateForm {
    title: String,
}

pub async fn create_presentation(
    State(state): State<Arc<AppState>>,
    Extension(PresentBase(base)): Extension<PresentBase>,
    jar: CookieJar,
    Form(form): Form<CreateForm>,
) -> Response {
    let account = match require_account(&state, &jar, &base, &base).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let title: String = form.title.trim().chars().take(200).collect();
    let title = if title.is_empty() { "Untitled presentation".to_string() } else { title };
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let inserted = sqlx::query(
        "INSERT INTO presentations (id, owner_id, title, description, visibility, document, created_at, updated_at) \
         VALUES (?, ?, ?, '', 'private', ?, ?, ?)",
    )
    .bind(&id)
    .bind(&account.id)
    .bind(&title)
    .bind(default_document().to_string())
    .bind(&now)
    .bind(&now)
    .execute(&*state.forum_db)
    .await;
    if inserted.is_err() {
        return Redirect::to(&format!("{base}?error=Could+not+create+presentation")).into_response();
    }
    Redirect::to(&format!("{base}/edit/{id}")).into_response()
}

fn presentation_payload(row: &PresentationRow, media: &[MediaItem], author: &str, is_owner: bool) -> Value {
    json!({
        "id": row.id,
        "title": row.title,
        "description": row.description,
        "visibility": row.visibility,
        "updated_at": row.updated_at,
        "author": author,
        "is_owner": is_owner,
        "document": serde_json::from_str::<Value>(&row.document).unwrap_or_else(|_| default_document()),
        "media": media,
    })
}

async fn author_name(state: &AppState, owner_id: &str) -> String {
    sqlx::query_scalar("SELECT display_name FROM presentation_accounts WHERE id = ?")
        .bind(owner_id)
        .fetch_optional(&*state.forum_db)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

pub async fn edit_page(
    State(state): State<Arc<AppState>>,
    Extension(PresentBase(base)): Extension<PresentBase>,
    jar: CookieJar,
    AxPath(id): AxPath<String>,
) -> Response {
    let account = match require_account(&state, &jar, &base, &format!("{base}/edit/{id}")).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let Some(row) = load_presentation(&state, &id).await.filter(|p| p.owner_id == account.id) else {
        return not_found();
    };
    let media = load_media(&state, &id).await;
    let mut data = presentation_payload(&row, &media, &account.display_name, true);
    data["base"] = json!(base);
    data["limits"] = json!({
        "image": IMAGE_EXTS, "video": VIDEO_EXTS, "audio": AUDIO_EXTS,
    });
    let mut ctx = tera::Context::new();
    ctx.insert("base", &base);
    ctx.insert("data_json", &json_for_script(&data));
    render(&state, "present-edit.html", &ctx)
}

pub async fn play_page(
    State(state): State<Arc<AppState>>,
    Extension(PresentBase(base)): Extension<PresentBase>,
    jar: CookieJar,
    AxPath(id): AxPath<String>,
) -> Response {
    let account = match require_account(&state, &jar, &base, &format!("{base}/play/{id}")).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let Some(row) = load_presentation(&state, &id).await.filter(|p| p.can_view(&account)) else {
        return not_found();
    };
    let media = load_media(&state, &id).await;
    let author = author_name(&state, &row.owner_id).await;
    let mut data = presentation_payload(&row, &media, &author, row.owner_id == account.id);
    data["base"] = json!(base);
    let mut ctx = tera::Context::new();
    ctx.insert("base", &base);
    ctx.insert("data_json", &json_for_script(&data));
    render(&state, "present-play.html", &ctx)
}

pub async fn delete_presentation(
    State(state): State<Arc<AppState>>,
    Extension(PresentBase(base)): Extension<PresentBase>,
    jar: CookieJar,
    AxPath(id): AxPath<String>,
) -> Response {
    let account = match require_account(&state, &jar, &base, &base).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    if load_presentation(&state, &id).await.filter(|p| p.owner_id == account.id).is_none() {
        return not_found();
    }
    delete_presentation_data(&state.forum_db, &id).await;
    Redirect::to(&format!("{base}?notice=Presentation+deleted")).into_response()
}

async fn delete_presentation_data(db: &ForumDb, id: &str) {
    let _ = sqlx::query("DELETE FROM presentation_media WHERE presentation_id = ?")
        .bind(id).execute(&**db).await;
    let _ = sqlx::query("DELETE FROM presentations WHERE id = ?")
        .bind(id).execute(&**db).await;
    let _ = fs::remove_dir_all(media_dir(id)).await;
}

// ─── JSON API ────────────────────────────────────────────────────────────────

pub async fn api_get(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    AxPath(id): AxPath<String>,
) -> Response {
    let Some(account) = current_account(&state, &jar).await else {
        return json_error(StatusCode::UNAUTHORIZED, "Not signed in");
    };
    let Some(row) = load_presentation(&state, &id).await.filter(|p| p.can_view(&account)) else {
        return json_error(StatusCode::NOT_FOUND, "Not found");
    };
    let media = load_media(&state, &id).await;
    let author = author_name(&state, &row.owner_id).await;
    Json(presentation_payload(&row, &media, &author, row.owner_id == account.id)).into_response()
}

#[derive(Deserialize)]
pub struct SaveRequest {
    title: String,
    #[serde(default)]
    description: String,
    visibility: String,
    document: Value,
}

pub async fn api_save(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    AxPath(id): AxPath<String>,
    Json(req): Json<SaveRequest>,
) -> Response {
    let Some(account) = current_account(&state, &jar).await else {
        return json_error(StatusCode::UNAUTHORIZED, "Your session expired. Sign in again in another tab, then save.");
    };
    if load_presentation(&state, &id).await.filter(|p| p.owner_id == account.id).is_none() {
        return json_error(StatusCode::NOT_FOUND, "Not found");
    }
    if let Err(e) = check_document(&req.document) {
        return json_error(StatusCode::BAD_REQUEST, &e);
    }
    let title: String = req.title.trim().chars().take(200).collect();
    let title = if title.is_empty() { "Untitled presentation".to_string() } else { title };
    let description: String = req.description.trim().chars().take(2000).collect();
    let visibility = if req.visibility == "shared" { "shared" } else { "private" };
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE presentations SET title = ?, description = ?, visibility = ?, document = ?, updated_at = ? \
         WHERE id = ? AND owner_id = ?",
    )
    .bind(&title)
    .bind(&description)
    .bind(visibility)
    .bind(req.document.to_string())
    .bind(&now)
    .bind(&id)
    .bind(&account.id)
    .execute(&*state.forum_db)
    .await;
    match result {
        Ok(_) => Json(json!({ "ok": true, "updated_at": now })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "Could not save"),
    }
}

// ─── Media ───────────────────────────────────────────────────────────────────

pub async fn upload_media(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    AxPath(id): AxPath<String>,
    mut multipart: Multipart,
) -> Response {
    let Some(account) = current_account(&state, &jar).await else {
        return json_error(StatusCode::UNAUTHORIZED, "Not signed in");
    };
    if load_presentation(&state, &id).await.filter(|p| p.owner_id == account.id).is_none() {
        return json_error(StatusCode::NOT_FOUND, "Not found");
    }
    let dir = media_dir(&id);
    if fs::create_dir_all(&dir).await.is_err() {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Could not create media folder");
    }
    let storage_limit = state.config.upload_storage_limit;

    let mut uploaded: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    loop {
        let mut field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                errors.push(format!("Upload interrupted: {e}"));
                break;
            }
        };
        let Some(original) = field.file_name().map(str::to_string) else { continue };
        let Some(filename) = clean_filename(&original) else {
            errors.push(format!("{original}: unsupported file type"));
            continue;
        };
        let kind = media_kind(&filename).unwrap_or("image");

        let tmp = dir.join(format!(".{}.part", Uuid::new_v4()));
        let Ok(mut file) = fs::File::create(&tmp).await else {
            errors.push(format!("{original}: could not write file"));
            continue;
        };
        let mut size: u64 = 0;
        let mut failed = false;
        loop {
            match field.chunk().await {
                Ok(Some(bytes)) => {
                    size += bytes.len() as u64;
                    if file.write_all(&bytes).await.is_err() {
                        failed = true;
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        let _ = file.flush().await;
        drop(file);
        if failed {
            let _ = fs::remove_file(&tmp).await;
            errors.push(format!("{original}: upload failed"));
            continue;
        }

        if let Some(limit) = storage_limit {
            let used: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(size), 0) FROM presentation_media \
                 WHERE NOT (presentation_id = ? AND filename = ?)",
            )
            .bind(&id)
            .bind(&filename)
            .fetch_one(&*state.forum_db)
            .await
            .unwrap_or(0);
            if used as u64 + size > limit {
                let _ = fs::remove_file(&tmp).await;
                errors.push(format!("{original}: media storage is full"));
                continue;
            }
        }

        if fs::rename(&tmp, dir.join(&filename)).await.is_err() {
            let _ = fs::remove_file(&tmp).await;
            errors.push(format!("{original}: could not save file"));
            continue;
        }
        let _ = sqlx::query(
            "INSERT INTO presentation_media (presentation_id, filename, kind, size, uploaded_at) \
             VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(presentation_id, filename) DO UPDATE SET \
             kind = excluded.kind, size = excluded.size, uploaded_at = excluded.uploaded_at",
        )
        .bind(&id)
        .bind(&filename)
        .bind(kind)
        .bind(size as i64)
        .bind(Utc::now().to_rfc3339())
        .execute(&*state.forum_db)
        .await;
        uploaded.push(filename);
    }

    let media = load_media(&state, &id).await;
    Json(json!({ "ok": errors.is_empty(), "uploaded": uploaded, "errors": errors, "media": media }))
        .into_response()
}

#[derive(Deserialize)]
pub struct DeleteMediaRequest {
    filename: String,
}

pub async fn delete_media(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    AxPath(id): AxPath<String>,
    Json(req): Json<DeleteMediaRequest>,
) -> Response {
    let Some(account) = current_account(&state, &jar).await else {
        return json_error(StatusCode::UNAUTHORIZED, "Not signed in");
    };
    if load_presentation(&state, &id).await.filter(|p| p.owner_id == account.id).is_none() {
        return json_error(StatusCode::NOT_FOUND, "Not found");
    }
    if clean_filename(&req.filename).as_deref() != Some(req.filename.as_str()) {
        return json_error(StatusCode::BAD_REQUEST, "Invalid filename");
    }
    let _ = sqlx::query("DELETE FROM presentation_media WHERE presentation_id = ? AND filename = ?")
        .bind(&id)
        .bind(&req.filename)
        .execute(&*state.forum_db)
        .await;
    let _ = fs::remove_file(media_dir(&id).join(&req.filename)).await;
    let media = load_media(&state, &id).await;
    Json(json!({ "ok": true, "media": media })).into_response()
}

pub async fn serve_media(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    AxPath((id, filename)): AxPath<(String, String)>,
    req: Request,
) -> Response {
    let Some(account) = current_account(&state, &jar).await else {
        return (StatusCode::UNAUTHORIZED, "Not signed in").into_response();
    };
    if load_presentation(&state, &id).await.filter(|p| p.can_view(&account)).is_none() {
        return not_found();
    }
    if clean_filename(&filename).as_deref() != Some(filename.as_str()) {
        return not_found();
    }
    // ServeFile handles Range requests, which video seeking depends on.
    match ServeFile::new(media_dir(&id).join(&filename)).oneshot(req).await {
        Ok(res) => {
            let mut res = res.map(Body::new);
            let headers = res.headers_mut();
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=3600"));
            headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
            res
        }
        Err(_) => not_found(),
    }
}

// ─── Admin dashboard support ─────────────────────────────────────────────────

pub async fn admin_account_rows(db: &ForumDb) -> Vec<AdminAccountRow> {
    sqlx::query_as::<_, AdminAccountRow>(
        "SELECT a.id, a.username, a.display_name, a.status, a.created_by, a.created_at, a.last_login_at, \
            (SELECT COUNT(*) FROM presentations p WHERE p.owner_id = a.id) AS presentation_count, \
            (SELECT COUNT(*) FROM presentations p WHERE p.owner_id = a.id AND p.visibility = 'shared') AS shared_count, \
            (SELECT COALESCE(SUM(m.size), 0) FROM presentation_media m \
                JOIN presentations p ON p.id = m.presentation_id WHERE p.owner_id = a.id) AS media_bytes \
         FROM presentation_accounts a ORDER BY a.username",
    )
    .fetch_all(&**db)
    .await
    .unwrap_or_default()
}

pub async fn admin_create_account(
    db: &ForumDb,
    username: &str,
    display_name: &str,
    password: &str,
    created_by: &str,
) -> Result<(), String> {
    let username = username.trim();
    if !valid_username(username) {
        return Err("Username must be 3-32 characters: letters, numbers, _ - .".into());
    }
    if password.len() < MIN_PASSWORD_LEN {
        return Err("Password must be at least 8 characters".into());
    }
    let display_name: String = display_name.trim().chars().take(64).collect();
    let display_name = if display_name.is_empty() { username.to_string() } else { display_name };
    let hashed = hash(password, DEFAULT_COST).map_err(|_| "Could not hash password".to_string())?;
    sqlx::query(
        "INSERT INTO presentation_accounts (id, username, display_name, password_hash, status, created_by, created_at) \
         VALUES (?, ?, ?, ?, 'active', ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(username)
    .bind(&display_name)
    .bind(&hashed)
    .bind(created_by)
    .bind(Utc::now().to_rfc3339())
    .execute(&**db)
    .await
    .map(|_| ())
    .map_err(|_| "That username is already taken".to_string())
}

pub async fn admin_reset_password(db: &ForumDb, id: &str, password: &str) -> Result<(), String> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err("Password must be at least 8 characters".into());
    }
    let hashed = hash(password, DEFAULT_COST).map_err(|_| "Could not hash password".to_string())?;
    sqlx::query("UPDATE presentation_accounts SET password_hash = ? WHERE id = ?")
        .bind(&hashed)
        .bind(id)
        .execute(&**db)
        .await
        .map_err(|_| "Could not update password".to_string())?;
    let _ = sqlx::query("DELETE FROM presentation_sessions WHERE account_id = ?")
        .bind(id)
        .execute(&**db)
        .await;
    Ok(())
}

pub async fn admin_set_status(db: &ForumDb, id: &str, active: bool) {
    let _ = sqlx::query("UPDATE presentation_accounts SET status = ? WHERE id = ?")
        .bind(if active { "active" } else { "suspended" })
        .bind(id)
        .execute(&**db)
        .await;
    if !active {
        let _ = sqlx::query("DELETE FROM presentation_sessions WHERE account_id = ?")
            .bind(id)
            .execute(&**db)
            .await;
    }
}

/// Removes the account along with every presentation and media file it owns.
pub async fn admin_delete_account(db: &ForumDb, id: &str) {
    let owned: Vec<String> = sqlx::query_scalar("SELECT id FROM presentations WHERE owner_id = ?")
        .bind(id)
        .fetch_all(&**db)
        .await
        .unwrap_or_default();
    for pid in owned {
        delete_presentation_data(db, &pid).await;
    }
    let _ = sqlx::query("DELETE FROM presentation_sessions WHERE account_id = ?")
        .bind(id).execute(&**db).await;
    let _ = sqlx::query("DELETE FROM presentation_accounts WHERE id = ?")
        .bind(id).execute(&**db).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_filename_normalizes_and_filters() {
        assert_eq!(clean_filename("My Photo (1).JPG").as_deref(), Some("My_Photo_1.jpg"));
        assert_eq!(clean_filename("../../etc/intro.webp").as_deref(), Some("intro.webp"));
        assert_eq!(clean_filename("C:\\users\\x\\clip.MP4").as_deref(), Some("clip.mp4"));
        assert_eq!(clean_filename("theme.ogg").as_deref(), Some("theme.ogg"));
        assert_eq!(clean_filename("page.html"), None);
        assert_eq!(clean_filename("vector.svg"), None);
        assert_eq!(clean_filename("noextension"), None);
        assert_eq!(clean_filename("###.png").as_deref(), Some("file.png"));
    }

    #[test]
    fn safe_next_stays_inside_base() {
        assert_eq!(safe_next(Some("/present/play/x"), "/present"), "/present/play/x");
        assert_eq!(safe_next(Some("https://evil.example/present"), "/present"), "/present");
        assert_eq!(safe_next(Some("/elsewhere"), "/present"), "/present");
        assert_eq!(safe_next(Some("/presentation-x"), "/present"), "/present");
        assert_eq!(safe_next(None, "/present"), "/present");
    }

    #[test]
    fn check_document_rejects_bad_ids() {
        assert!(check_document(&default_document()).is_ok());
        assert!(check_document(&json!({ "pages": [] })).is_err());
        assert!(check_document(&json!({ "pages": [{ "id": "a" }, { "id": "a" }] })).is_err());
        assert!(check_document(&json!({ "pages": [{ "id": "bad id" }] })).is_err());
    }

    #[test]
    fn json_for_script_cannot_close_tag() {
        let s = json_for_script(&json!({ "t": "</script><b>" }));
        assert!(!s.contains('<'));
    }
}
