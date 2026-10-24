// Private media: a page plus a directory of audio/video/image files visible
// only to accounts created from the admin dashboard. There is deliberately no
// registration route and no email: an owner makes a username + password and
// hands it to the person. Mounted with a
// `["static/private.html", "private", "private"]` route in config.toml — the
// middle entry is the media directory, which must live outside every publicly
// served directory (static/, uploads/, ...) or the mount is refused.
//
// Access is enforced by this module on every request (page and files), not by
// auth_guard rules, so the area stays closed even if no access rule covers it.

use axum::{
    Extension, Router,
    body::Body,
    extract::{Form, Path as AxPath, Query, Request, State},
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
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;
use tower::ServiceExt;
use tower_http::services::ServeFile;
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "private_session";
const SESSION_DAYS: i64 = 30;
const MIN_PASSWORD_LEN: usize = 8;
const MAX_FAILURES: u32 = 5;
const LOCKOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Subdirectories created inside the media directory and listed on the page.
const SECTIONS: &[(&str, &str)] = &[("image", "Images"), ("video", "Video"), ("audio", "Audio")];
const MAX_DEPTH: usize = 8;

/// Directories webify always serves publicly, regardless of config routes.
const PUBLIC_DIRS: &[&str] = &["static", "templates", "uploads", "css", "styles", "js", "scripts", "images"];

/// Failed sign-ins per lowercase username: (count, time of first failure).
static FAILURES: LazyLock<Mutex<HashMap<String, (u32, Instant)>>> = LazyLock::new(Default::default);

#[derive(Clone)]
struct PrivateMount {
    base: String,
    template: String,
    root: PathBuf,
}

pub async fn init_tables(db: &ForumDb) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS private_accounts (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE COLLATE NOCASE,
            password_hash TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_by TEXT NOT NULL,
            created_at TEXT NOT NULL,
            last_login_at TEXT
        )",
    ).execute(&**db).await.unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS private_sessions (
            token TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL
        )",
    ).execute(&**db).await.unwrap();
}

/// Build the router for a private mount, or None if the media directory is
/// unsafe (reachable through a public mount) or can't be created.
pub fn router(
    base: &str,
    template_path: &str,
    media_dir: &str,
    sites: &HashMap<String, Vec<(String, Vec<String>)>>,
) -> Option<Router<Arc<AppState>>> {
    let root_raw = Path::new(media_dir);
    for (dir, _) in SECTIONS {
        if let Err(e) = std::fs::create_dir_all(root_raw.join(dir)) {
            eprintln!("private media: cannot create {media_dir}/{dir}: {e} — {base} not mounted");
            return None;
        }
    }
    let root = match root_raw.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("private media: cannot resolve {media_dir}: {e} — {base} not mounted");
            return None;
        }
    };
    if let Some(public) = overlapping_public_dir(&root, media_dir, sites) {
        eprintln!(
            "private media: {media_dir} overlaps publicly served directory {} — {base} NOT mounted. \
             Move it outside every public directory (e.g. next to static/, not inside it).",
            public.display()
        );
        return None;
    }
    let mount = PrivateMount {
        base: base.trim_end_matches('/').to_string(),
        template: template_path.trim_start_matches("static/").to_string(),
        root,
    };
    Some(
        Router::new()
            .route("/", get(index))
            .route("/login", get(login_form).post(login))
            .route("/logout", get(logout))
            .route("/password", post(change_password))
            .route("/files/{*rest}", get(serve_file))
            .layer(Extension(mount)),
    )
}

/// Returns a publicly served directory that contains, or is contained by,
/// the private root. Any directory named in a config route counts as public
/// except this mount's own media directory.
fn overlapping_public_dir(
    root: &Path,
    media_dir: &str,
    sites: &HashMap<String, Vec<(String, Vec<String>)>>,
) -> Option<PathBuf> {
    let mut candidates: Vec<String> = PUBLIC_DIRS.iter().map(|s| s.to_string()).collect();
    for routes in sites.values() {
        for (_, settings) in routes {
            let is_this_mount = settings.get(1).map(String::as_str) == Some(media_dir)
                && settings.get(2).map(String::as_str) == Some("private");
            if !is_this_mount {
                candidates.extend(settings.iter().cloned());
            }
        }
    }
    candidates.into_iter().find_map(|c| {
        let p = Path::new(&c).canonicalize().ok()?;
        (p.is_dir() && (root.starts_with(&p) || p.starts_with(root))).then_some(p)
    })
}

// ─── Accounts & sessions ─────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct Account {
    id: String,
    username: String,
    password_hash: String,
    status: String,
}

fn valid_username(username: &str) -> bool {
    (3..=32).contains(&username.len())
        && username.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
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

/// Only allow redirecting back into the private area after login.
fn safe_next(next: Option<&str>, base: &str) -> String {
    match next {
        Some(n) if (n == base || n.starts_with(&format!("{base}/")))
            && !n.contains("://") && !n.contains('\\') => n.to_string(),
        _ => base.to_string(),
    }
}

async fn current_account(state: &AppState, jar: &CookieJar) -> Option<Account> {
    let token = jar.get(SESSION_COOKIE)?.value().to_string();
    let (account_id, expires): (String, String) = sqlx::query_as(
        "SELECT account_id, expires_at FROM private_sessions WHERE token = ?",
    )
    .bind(&token)
    .fetch_optional(&*state.forum_db)
    .await
    .ok()??;
    let expires = chrono::DateTime::parse_from_rfc3339(&expires).ok()?;
    if expires < Utc::now() {
        let _ = sqlx::query("DELETE FROM private_sessions WHERE token = ?")
            .bind(&token)
            .execute(&*state.forum_db)
            .await;
        return None;
    }
    sqlx::query_as::<_, Account>(
        "SELECT id, username, password_hash, status FROM private_accounts WHERE id = ? AND status = 'active'",
    )
    .bind(&account_id)
    .fetch_optional(&*state.forum_db)
    .await
    .ok()
    .flatten()
}

fn locked_out(username: &str) -> bool {
    let mut map = FAILURES.lock().unwrap();
    match map.get(username) {
        Some((count, since)) if since.elapsed() < LOCKOUT => *count >= MAX_FAILURES,
        Some(_) => {
            map.remove(username);
            false
        }
        None => false,
    }
}

fn record_failure(username: &str) {
    let mut map = FAILURES.lock().unwrap();
    let entry = map.entry(username.to_string()).or_insert((0, Instant::now()));
    if entry.1.elapsed() >= LOCKOUT {
        *entry = (0, Instant::now());
    }
    entry.0 += 1;
}

/// Headers applied to every response from a private mount.
fn harden(mut res: Response) -> Response {
    let h = res.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    h.insert("x-robots-tag", HeaderValue::from_static("noindex, nofollow, noarchive"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    res
}

fn render(state: &AppState, template: &str, ctx: &tera::Context) -> Response {
    harden(match state.tera.render(template, ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Template error rendering {template}: {e}")).into_response(),
    })
}

fn to_login(base: &str, next: &str) -> Response {
    harden(Redirect::to(&format!("{base}/login?next={}", urlencoding::encode(next))).into_response())
}

// ─── Handlers ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LoginQuery {
    next: Option<String>,
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
    next: Option<String>,
}

#[derive(Deserialize)]
struct ChangePasswordForm {
    current_password: String,
    new_password: String,
    confirm_password: String,
}

#[derive(Deserialize, Default)]
struct IndexQuery {
    notice: Option<String>,
    error: Option<String>,
}

async fn login_form(
    State(state): State<Arc<AppState>>,
    Extension(mount): Extension<PrivateMount>,
    jar: CookieJar,
    Query(q): Query<LoginQuery>,
) -> Response {
    let next = safe_next(q.next.as_deref(), &mount.base);
    if current_account(&state, &jar).await.is_some() {
        return harden(Redirect::to(&next).into_response());
    }
    let mut ctx = tera::Context::new();
    ctx.insert("base", &mount.base);
    ctx.insert("next", &next);
    render(&state, "private-login.html", &ctx)
}

async fn login(
    State(state): State<Arc<AppState>>,
    Extension(mount): Extension<PrivateMount>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> Response {
    let next = safe_next(form.next.as_deref(), &mount.base);
    let username = form.username.trim();
    let key = username.to_ascii_lowercase();
    let fail = |msg: &str| {
        let mut ctx = tera::Context::new();
        ctx.insert("base", &mount.base);
        ctx.insert("next", &next);
        ctx.insert("username", username);
        ctx.insert("error", msg);
        render(&state, "private-login.html", &ctx)
    };
    if locked_out(&key) {
        return fail("Too many failed attempts. Try again in 15 minutes.");
    }

    let account: Option<Account> = sqlx::query_as(
        "SELECT id, username, password_hash, status FROM private_accounts WHERE username = ?",
    )
    .bind(username)
    .fetch_optional(&*state.forum_db)
    .await
    .ok()
    .flatten();
    let ok = account.as_ref().is_some_and(|a| {
        a.status == "active" && verify(&form.password, &a.password_hash).unwrap_or(false)
    });
    let Some(account) = account.filter(|_| ok) else {
        record_failure(&key);
        return fail("Invalid username or password.");
    };
    FAILURES.lock().unwrap().remove(&key);

    let token = Uuid::new_v4().to_string();
    let now = Utc::now();
    let _ = sqlx::query(
        "INSERT INTO private_sessions (token, account_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&token)
    .bind(&account.id)
    .bind(now.to_rfc3339())
    .bind((now + Duration::days(SESSION_DAYS)).to_rfc3339())
    .execute(&*state.forum_db)
    .await;
    let _ = sqlx::query("UPDATE private_accounts SET last_login_at = ? WHERE id = ?")
        .bind(now.to_rfc3339())
        .bind(&account.id)
        .execute(&*state.forum_db)
        .await;

    let cookie = session_cookie(token, &mount.base, state.config.ssl_enabled);
    harden((jar.add(cookie), Redirect::to(&next)).into_response())
}

async fn logout(
    State(state): State<Arc<AppState>>,
    Extension(mount): Extension<PrivateMount>,
    jar: CookieJar,
) -> Response {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        let _ = sqlx::query("DELETE FROM private_sessions WHERE token = ?")
            .bind(c.value())
            .execute(&*state.forum_db)
            .await;
    }
    let cookie = Cookie::build(SESSION_COOKIE).path(mount.base.clone()).build();
    harden((jar.remove(cookie), Redirect::to(&format!("{}/login", mount.base))).into_response())
}

async fn change_password(
    State(state): State<Arc<AppState>>,
    Extension(mount): Extension<PrivateMount>,
    jar: CookieJar,
    Form(form): Form<ChangePasswordForm>,
) -> Response {
    let base = &mount.base;
    let Some(account) = current_account(&state, &jar).await else {
        return to_login(base, base);
    };
    let fail = |msg: &str| harden(Redirect::to(&format!("{base}?error={}", urlencoding::encode(msg))).into_response());
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
    let _ = sqlx::query("UPDATE private_accounts SET password_hash = ? WHERE id = ?")
        .bind(&hashed)
        .bind(&account.id)
        .execute(&*state.forum_db)
        .await;
    // Sign out every other device that was using the old password.
    let current = jar.get(SESSION_COOKIE).map(|c| c.value().to_string()).unwrap_or_default();
    let _ = sqlx::query("DELETE FROM private_sessions WHERE account_id = ? AND token != ?")
        .bind(&account.id)
        .bind(&current)
        .execute(&*state.forum_db)
        .await;
    harden(Redirect::to(&format!("{base}?notice=Password+updated")).into_response())
}

#[derive(Serialize)]
struct MediaFile {
    name: String,
    url: String,
    size: u64,
}

#[derive(Serialize)]
struct Section {
    key: &'static str,
    title: &'static str,
    files: Vec<MediaFile>,
}

async fn index(
    State(state): State<Arc<AppState>>,
    Extension(mount): Extension<PrivateMount>,
    jar: CookieJar,
    Query(q): Query<IndexQuery>,
) -> Response {
    let Some(account) = current_account(&state, &jar).await else {
        return to_login(&mount.base, &mount.base);
    };

    let mut sections = Vec::new();
    for (key, title) in SECTIONS {
        let mut files = Vec::new();
        collect_files(&mount.root.join(key), key, &mount.base, 0, &mut files);
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        sections.push(Section { key, title, files });
    }

    let mut ctx = tera::Context::new();
    ctx.insert("username", &account.username);
    ctx.insert("base", &mount.base);
    ctx.insert("sections", &sections);
    ctx.insert("notice", &q.notice.unwrap_or_default());
    ctx.insert("error", &q.error.unwrap_or_default());
    render(&state, &mount.template, &ctx)
}

/// Recursively list visible files under `dir`. `rel` is the path of `dir`
/// relative to the media root, used to build each file's URL.
fn collect_files(dir: &Path, rel: &str, base: &str, depth: usize, out: &mut Vec<MediaFile>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let child_rel = format!("{rel}/{name}");
        if meta.is_dir() {
            collect_files(&entry.path(), &child_rel, base, depth + 1, out);
        } else if meta.is_file() {
            let url_path: Vec<String> = child_rel.split('/').map(|s| urlencoding::encode(s).into_owned()).collect();
            out.push(MediaFile {
                // Show the path below the section folder, e.g. "album/01.jpg".
                name: child_rel.split_once('/').map(|(_, n)| n).unwrap_or(&name).to_string(),
                url: format!("{}/files/{}", base, url_path.join("/")),
                size: meta.len(),
            });
        }
    }
}

/// Resolve a request path below the media root, rejecting traversal, hidden
/// files, and symlinks that point outside the root.
fn resolve(root: &Path, rest: &str) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for seg in rest.split('/') {
        if seg.is_empty() || seg.starts_with('.') || seg.contains('\\') || seg.contains('\0') {
            return None;
        }
        path.push(seg);
    }
    let real = path.canonicalize().ok()?;
    (real.starts_with(root) && real.is_file()).then_some(real)
}

async fn serve_file(
    State(state): State<Arc<AppState>>,
    Extension(mount): Extension<PrivateMount>,
    jar: CookieJar,
    AxPath(rest): AxPath<String>,
    req: Request,
) -> Response {
    // No redirect for media requests: a bare 401 keeps the login page out of
    // <img>/<video> tags and tells a direct visitor nothing.
    if current_account(&state, &jar).await.is_none() {
        return harden((StatusCode::UNAUTHORIZED, "Not signed in").into_response());
    }
    let not_found = || harden((StatusCode::NOT_FOUND, "Not found").into_response());
    let Some(file) = resolve(&mount.root, &rest) else {
        return not_found();
    };
    // ServeFile handles Range requests, which audio/video seeking depends on.
    match ServeFile::new(file).oneshot(req).await {
        Ok(res) => harden(res.map(Body::new)),
        Err(_) => not_found(),
    }
}

// ─── Admin dashboard support ─────────────────────────────────────────────────

#[derive(Serialize, sqlx::FromRow)]
pub struct AdminAccountRow {
    pub id: String,
    pub username: String,
    pub status: String,
    pub created_by: String,
    pub created_at: String,
    pub last_login_at: Option<String>,
}

pub async fn admin_account_rows(db: &ForumDb) -> Vec<AdminAccountRow> {
    sqlx::query_as::<_, AdminAccountRow>(
        "SELECT id, username, status, created_by, created_at, last_login_at FROM private_accounts ORDER BY username",
    )
    .fetch_all(&**db)
    .await
    .unwrap_or_default()
}

pub async fn admin_create_account(db: &ForumDb, username: &str, password: &str, created_by: &str) -> Result<(), String> {
    let username = username.trim();
    if !valid_username(username) {
        return Err("Username must be 3-32 characters: letters, numbers, _ - .".into());
    }
    if password.len() < MIN_PASSWORD_LEN {
        return Err("Password must be at least 8 characters".into());
    }
    let hashed = hash(password, DEFAULT_COST).map_err(|_| "Could not hash password".to_string())?;
    sqlx::query(
        "INSERT INTO private_accounts (id, username, password_hash, status, created_by, created_at) \
         VALUES (?, ?, ?, 'active', ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(username)
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
    sqlx::query("UPDATE private_accounts SET password_hash = ? WHERE id = ?")
        .bind(&hashed)
        .bind(id)
        .execute(&**db)
        .await
        .map_err(|_| "Could not update password".to_string())?;
    let _ = sqlx::query("DELETE FROM private_sessions WHERE account_id = ?")
        .bind(id)
        .execute(&**db)
        .await;
    Ok(())
}

pub async fn admin_set_status(db: &ForumDb, id: &str, active: bool) {
    let _ = sqlx::query("UPDATE private_accounts SET status = ? WHERE id = ?")
        .bind(if active { "active" } else { "suspended" })
        .bind(id)
        .execute(&**db)
        .await;
    if !active {
        let _ = sqlx::query("DELETE FROM private_sessions WHERE account_id = ?")
            .bind(id)
            .execute(&**db)
            .await;
    }
}

pub async fn admin_delete_account(db: &ForumDb, id: &str) {
    let _ = sqlx::query("DELETE FROM private_sessions WHERE account_id = ?")
        .bind(id).execute(&**db).await;
    let _ = sqlx::query("DELETE FROM private_accounts WHERE id = ?")
        .bind(id).execute(&**db).await;
}

/// The first configured private mount point, for linking from the dashboard.
pub fn configured_path(config: &crate::config::Config) -> Option<String> {
    config.sites.values().flatten().find_map(|(path, settings)| {
        (settings.get(2).map(String::as_str) == Some("private"))
            .then(|| path.trim_end_matches('/').to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_rejects_escapes() {
        let tmp = std::env::temp_dir().join(format!("webify-private-test-{}", std::process::id()));
        let root = tmp.join("private");
        std::fs::create_dir_all(root.join("image")).unwrap();
        std::fs::write(root.join("image/a.jpg"), b"x").unwrap();
        std::fs::write(root.join("image/.hidden"), b"x").unwrap();
        std::fs::write(tmp.join("outside.txt"), b"x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.join("outside.txt"), root.join("image/link.txt")).unwrap();
        let root = root.canonicalize().unwrap();

        assert!(resolve(&root, "image/a.jpg").is_some());
        assert!(resolve(&root, "image/../../outside.txt").is_none());
        assert!(resolve(&root, "../outside.txt").is_none());
        assert!(resolve(&root, "image//a.jpg").is_none());
        assert!(resolve(&root, "image/.hidden").is_none());
        assert!(resolve(&root, "image").is_none());
        assert!(resolve(&root, "image\\..\\..\\outside.txt").is_none());
        #[cfg(unix)]
        assert!(resolve(&root, "image/link.txt").is_none());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn next_stays_inside_mount() {
        assert_eq!(safe_next(Some("/private/files/image/a.jpg"), "/private"), "/private/files/image/a.jpg");
        assert_eq!(safe_next(Some("https://evil.example/private"), "/private"), "/private");
        assert_eq!(safe_next(Some("//evil.example"), "/private"), "/private");
        assert_eq!(safe_next(Some("/privatex"), "/private"), "/private");
        assert_eq!(safe_next(None, "/private"), "/private");
    }

    #[test]
    fn lockout_after_repeated_failures() {
        let user = "lockout-test-user";
        for _ in 0..MAX_FAILURES - 1 {
            record_failure(user);
        }
        assert!(!locked_out(user));
        record_failure(user);
        assert!(locked_out(user));
        FAILURES.lock().unwrap().remove(user);
    }
}
