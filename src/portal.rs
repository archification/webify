// Self-service hospital/client accounts: registration + email verification +
// a per-account document locker (upload/list/download). Separate from the
// forum's `users` table (internal members) and from `auth_guard` (employee
// Google SSO) — this is the public "create an account" surface.

use axum::{
    extract::{State, Form, Query, Path as AxPath, Multipart},
    response::{Html, IntoResponse, Redirect, Response},
    http::{StatusCode, HeaderMap, header},
};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use crate::AppState;
use crate::forum::ForumDb;
use bcrypt::{hash, verify, DEFAULT_COST};
use uuid::Uuid;
use chrono::{Utc, Duration};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::path::Path as FsPath;
use tokio::fs;

pub const PORTAL_SESSION_COOKIE: &str = "portal_session";
const DEFAULT_PORTAL_ACCOUNT_QUOTA: u64 = 500 * 1024 * 1024; // 500 MB per hospital

pub async fn init_portal_tables(db: &ForumDb) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS portal_accounts (
            id TEXT PRIMARY KEY,
            organization TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            is_verified INTEGER NOT NULL DEFAULT 0,
            verification_token TEXT NOT NULL,
            created_at TEXT NOT NULL,
            npi TEXT,
            status TEXT NOT NULL DEFAULT 'active'
        )"
    ).execute(&**db).await.unwrap();

    // Migration for pre-existing databases created before the `npi`/`status`
    // columns existed. SQLite has no `ADD COLUMN IF NOT EXISTS`, so just
    // ignore the "duplicate column" error on a DB that already has them.
    let _ = sqlx::query("ALTER TABLE portal_accounts ADD COLUMN npi TEXT")
        .execute(&**db)
        .await;
    let _ = sqlx::query("ALTER TABLE portal_accounts ADD COLUMN status TEXT NOT NULL DEFAULT 'active'")
        .execute(&**db)
        .await;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS portal_sessions (
            token TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL
        )"
    ).execute(&**db).await.unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS portal_documents (
            id TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            original_filename TEXT NOT NULL,
            stored_path TEXT NOT NULL,
            size INTEGER NOT NULL,
            uploaded_at TEXT NOT NULL
        )"
    ).execute(&**db).await.unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS portal_password_resets (
            token TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            used INTEGER NOT NULL DEFAULT 0
        )"
    ).execute(&**db).await.unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS portal_email_changes (
            token TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            new_email TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            used INTEGER NOT NULL DEFAULT 0
        )"
    ).execute(&**db).await.unwrap();
}

#[derive(sqlx::FromRow, Clone)]
pub struct PortalAccount {
    pub id: String,
    pub organization: String,
    pub email: String,
    pub password_hash: String,
    pub is_verified: bool,
    #[allow(dead_code)]
    pub verification_token: String,
    #[allow(dead_code)]
    pub created_at: String,
    pub npi: Option<String>,
    pub status: String,
}

#[derive(sqlx::FromRow, Clone)]
pub struct PortalDocument {
    pub id: String,
    pub account_id: String,
    pub original_filename: String,
    pub stored_path: String,
    pub size: i64,
    pub uploaded_at: String,
}

#[derive(Serialize)]
struct DocumentView {
    id: String,
    original_filename: String,
    size_display: String,
    uploaded_at: String,
}

#[derive(Deserialize)]
pub struct RegisterForm {
    pub organization: String,
    pub email: String,
    pub password: String,
    pub confirm_password: String,
    #[serde(default)]
    pub npi: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct VerifyQuery {
    pub token: String,
}

#[derive(Deserialize, Default)]
pub struct LoginQuery {
    pub verified: Option<String>,
    pub reset: Option<String>,
    pub email_updated: Option<String>,
}

#[derive(Deserialize)]
pub struct ForgotPasswordForm {
    pub email: String,
}

#[derive(Deserialize)]
pub struct ResetPasswordQuery {
    pub token: String,
}

#[derive(Deserialize)]
pub struct ResetPasswordForm {
    pub token: String,
    pub password: String,
    pub confirm_password: String,
}

#[derive(Deserialize)]
pub struct UpdateProfileForm {
    pub organization: String,
    #[serde(default)]
    pub npi: Option<String>,
}

#[derive(Deserialize)]
pub struct ChangePasswordForm {
    pub current_password: String,
    pub new_password: String,
    pub confirm_new_password: String,
}

#[derive(Deserialize)]
pub struct ChangeEmailForm {
    pub new_email: String,
    pub current_password: String,
}

#[derive(Deserialize)]
pub struct ConfirmEmailQuery {
    pub token: String,
}

#[derive(Deserialize, Default)]
pub struct ProfileQuery {
    pub updated: Option<String>,
    pub password_changed: Option<String>,
    pub email_requested: Option<String>,
    pub email_updated: Option<String>,
}

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

fn format_size(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}

/// Send the verification email for a newly-registered portal account. Reuses
/// the same SMTP config (`smtp_server`/`smtp_username`/`smtp_password`/`email_from`)
/// already used by `forum::send_verification_email`.
pub async fn send_verification_email(
    config: &crate::config::Config,
    to_email: &str,
    token: &str,
) -> Result<(), String> {
    let smtp_server = config.smtp_server.as_deref().unwrap_or("localhost");
    let smtp_port = config.smtp_port.unwrap_or(25);
    let domain = if config.domain.is_empty() {
        "localhost"
    } else {
        config.domain.as_str()
    };
    let scheme = if config.ssl_enabled { "https" } else { "http" };
    let site_name = config.site_name.as_str();

    let email = Message::builder()
        .from(
            config
                .email_from
                .as_deref()
                .unwrap_or("no-reply@webify.local")
                .parse()
                .map_err(|e| format!("{}", e))?,
        )
        .to(to_email.parse().map_err(|e| format!("{}", e))?)
        .subject(format!("Verify your {site_name} client portal account"))
        .body(format!(
            "Thanks for creating a {site_name} client portal account.\n\n\
             Please verify your email by clicking the link below:\n\n\
             {scheme}://{domain}/portal/verify?token={token}\n\n\
             You won't be able to upload documents until your email is verified.\n\n\
             If you didn't request this, you can ignore this email.",
            site_name = site_name,
            scheme = scheme,
            domain = domain,
            token = token,
        ))
        .map_err(|e| e.to_string())?;

    // `relay()` is implicit-TLS (port 465); `starttls_relay()` is explicit STARTTLS
    // (port 587, what Google's SMTP relay service and Gmail SMTP both expect).
    // Credentials are optional so this also works with the Workspace SMTP relay
    // service's IP-allowlist mode, which authenticates by source IP, not login.
    let mut builder = SmtpTransport::starttls_relay(smtp_server)
        .map_err(|e| e.to_string())?
        .port(smtp_port);
    if let (Some(user), Some(pass)) = (&config.smtp_username, &config.smtp_password) {
        builder = builder.credentials(Credentials::new(user.clone(), pass.clone()));
    }
    let mailer = builder.build();
    mailer.send(&email).map_err(|e| e.to_string())?;
    Ok(())
}

/// Send the password reset email for an existing portal account. Reuses the
/// same SMTP config as `send_verification_email`.
pub async fn send_password_reset_email(
    config: &crate::config::Config,
    to_email: &str,
    token: &str,
) -> Result<(), String> {
    let smtp_server = config.smtp_server.as_deref().unwrap_or("localhost");
    let smtp_port = config.smtp_port.unwrap_or(25);
    let domain = if config.domain.is_empty() {
        "localhost"
    } else {
        config.domain.as_str()
    };
    let scheme = if config.ssl_enabled { "https" } else { "http" };
    let site_name = config.site_name.as_str();

    let email = Message::builder()
        .from(
            config
                .email_from
                .as_deref()
                .unwrap_or("no-reply@webify.local")
                .parse()
                .map_err(|e| format!("{}", e))?,
        )
        .to(to_email.parse().map_err(|e| format!("{}", e))?)
        .subject(format!("Reset your {site_name} client portal password"))
        .body(format!(
            "We received a request to reset the password for your {site_name}\n\
             client portal account.\n\n\
             Click the link below to choose a new password. This link expires in\n\
             1 hour and can only be used once:\n\n\
             {scheme}://{domain}/portal/reset-password?token={token}\n\n\
             If you didn't request this, you can safely ignore this email — your\n\
             password will not be changed.",
            site_name = site_name,
            scheme = scheme,
            domain = domain,
            token = token,
        ))
        .map_err(|e| e.to_string())?;

    let mut builder = SmtpTransport::starttls_relay(smtp_server)
        .map_err(|e| e.to_string())?
        .port(smtp_port);
    if let (Some(user), Some(pass)) = (&config.smtp_username, &config.smtp_password) {
        builder = builder.credentials(Credentials::new(user.clone(), pass.clone()));
    }
    let mailer = builder.build();
    mailer.send(&email).map_err(|e| e.to_string())?;
    Ok(())
}

/// Send the confirmation email for an in-progress email-address change on an
/// existing portal account. The account's email on file is not updated until
/// this link is clicked, so a typo'd address just leaves the change pending
/// rather than locking the account out.
pub async fn send_email_change_email(
    config: &crate::config::Config,
    to_email: &str,
    token: &str,
) -> Result<(), String> {
    let smtp_server = config.smtp_server.as_deref().unwrap_or("localhost");
    let smtp_port = config.smtp_port.unwrap_or(25);
    let domain = if config.domain.is_empty() {
        "localhost"
    } else {
        config.domain.as_str()
    };
    let scheme = if config.ssl_enabled { "https" } else { "http" };
    let site_name = config.site_name.as_str();

    let email = Message::builder()
        .from(
            config
                .email_from
                .as_deref()
                .unwrap_or("no-reply@webify.local")
                .parse()
                .map_err(|e| format!("{}", e))?,
        )
        .to(to_email.parse().map_err(|e| format!("{}", e))?)
        .subject(format!("Confirm your new {site_name} client portal email"))
        .body(format!(
            "We received a request to change the email address on your {site_name}\n\
             client portal account to this address.\n\n\
             Click the link below to confirm the change. This link expires in 1 hour:\n\n\
             {scheme}://{domain}/portal/profile/email/confirm?token={token}\n\n\
             If you didn't request this, you can safely ignore this email — your\n\
             account email will not change.",
            site_name = site_name,
            scheme = scheme,
            domain = domain,
            token = token,
        ))
        .map_err(|e| e.to_string())?;

    let mut builder = SmtpTransport::starttls_relay(smtp_server)
        .map_err(|e| e.to_string())?
        .port(smtp_port);
    if let (Some(user), Some(pass)) = (&config.smtp_username, &config.smtp_password) {
        builder = builder.credentials(Credentials::new(user.clone(), pass.clone()));
    }
    let mailer = builder.build();
    mailer.send(&email).map_err(|e| e.to_string())?;
    Ok(())
}

async fn create_session(db: &ForumDb, account_id: &str) -> String {
    let token = Uuid::new_v4().to_string();
    let now = Utc::now();
    let expires = now + Duration::days(30);
    let _ = sqlx::query(
        "INSERT INTO portal_sessions (token, account_id, created_at, expires_at) VALUES (?, ?, ?, ?)"
    )
    .bind(&token)
    .bind(account_id)
    .bind(now.to_rfc3339())
    .bind(expires.to_rfc3339())
    .execute(&**db)
    .await;
    token
}

pub(crate) async fn get_current_account(state: &AppState, jar: &CookieJar) -> Option<PortalAccount> {
    let token = jar.get(PORTAL_SESSION_COOKIE)?.value().to_string();
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT account_id, expires_at FROM portal_sessions WHERE token = ?"
    )
    .bind(&token)
    .fetch_optional(&*state.forum_db)
    .await
    .ok()?;
    let (account_id, expires_str) = row?;
    let expires = chrono::DateTime::parse_from_rfc3339(&expires_str).ok()?;
    if expires < Utc::now() {
        let _ = sqlx::query("DELETE FROM portal_sessions WHERE token = ?")
            .bind(&token)
            .execute(&*state.forum_db)
            .await;
        return None;
    }
    sqlx::query_as("SELECT * FROM portal_accounts WHERE id = ?")
        .bind(&account_id)
        .fetch_optional(&*state.forum_db)
        .await
        .ok()
        .flatten()
}

fn session_cookie_builder(cookie: impl Into<Cookie<'static>>, state: &AppState) -> Cookie<'static> {
    let mut builder = Cookie::build(cookie).path("/").http_only(true);
    if state.config.ssl_enabled {
        builder = builder.secure(true);
    }
    builder.build()
}

// ─── Handlers ────────────────────────────────────────────────────────────────

pub async fn register_form(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let ctx = tera::Context::new();
    render(&state, "portal-register.html", &ctx)
}

pub async fn register(
    State(state): State<Arc<AppState>>,
    Form(form): Form<RegisterForm>,
) -> impl IntoResponse {
    let mut ctx = tera::Context::new();
    ctx.insert("organization", &form.organization);
    ctx.insert("email", &form.email);
    let npi_input = form.npi.as_deref().unwrap_or("").trim().to_string();
    ctx.insert("npi", &npi_input);

    if form.password != form.confirm_password {
        ctx.insert("error", "Passwords do not match.");
        return render(&state, "portal-register.html", &ctx);
    }
    if form.password.len() < 8 {
        ctx.insert("error", "Password must be at least 8 characters.");
        return render(&state, "portal-register.html", &ctx);
    }
    if form.organization.trim().is_empty() {
        ctx.insert("error", "Organization / hospital name is required.");
        return render(&state, "portal-register.html", &ctx);
    }
    if !npi_input.is_empty() && !crate::npi::is_valid_format(&npi_input) {
        ctx.insert("error", "NPI number must be exactly 10 digits.");
        return render(&state, "portal-register.html", &ctx);
    }

    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM portal_accounts WHERE email = ?")
        .bind(&form.email)
        .fetch_one(&*state.forum_db)
        .await
        .unwrap_or(0);
    if existing > 0 {
        ctx.insert("error", "An account with this email already exists.");
        return render(&state, "portal-register.html", &ctx);
    }

    let id = Uuid::new_v4().to_string();
    let token = Uuid::new_v4().to_string();
    let hashed = match hash(&form.password, DEFAULT_COST) {
        Ok(h) => h,
        Err(_) => {
            ctx.insert("error", "Failed to process password. Please try again.");
            return render(&state, "portal-register.html", &ctx);
        }
    };

    let npi_to_store = if npi_input.is_empty() { None } else { Some(npi_input.clone()) };

    let inserted = sqlx::query(
        "INSERT INTO portal_accounts (id, organization, email, password_hash, is_verified, verification_token, created_at, npi)
         VALUES (?, ?, ?, ?, 0, ?, ?, ?)"
    )
    .bind(&id)
    .bind(form.organization.trim())
    .bind(&form.email)
    .bind(&hashed)
    .bind(&token)
    .bind(Utc::now().to_rfc3339())
    .bind(&npi_to_store)
    .execute(&*state.forum_db)
    .await;

    if inserted.is_err() {
        ctx.insert("error", "An account with this email already exists.");
        return render(&state, "portal-register.html", &ctx);
    }

    if let Err(e) = send_verification_email(&state.config, &form.email, &token).await {
        eprintln!("Failed to send portal verification email to {}: {}", form.email, e);
    }

    let mut ctx = tera::Context::new();
    ctx.insert("email", &form.email);
    render(&state, "portal-check-email.html", &ctx)
}

pub async fn verify_email(
    State(state): State<Arc<AppState>>,
    Query(query): Query<VerifyQuery>,
) -> impl IntoResponse {
    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM portal_accounts WHERE verification_token = ?")
        .bind(&query.token)
        .fetch_optional(&*state.forum_db)
        .await
        .unwrap_or(None);

    if let Some((id,)) = row {
        let _ = sqlx::query("UPDATE portal_accounts SET is_verified = 1 WHERE id = ?")
            .bind(&id)
            .execute(&*state.forum_db)
            .await;
        Redirect::to("/portal/login?verified=1").into_response()
    } else {
        let mut ctx = tera::Context::new();
        ctx.insert("error", "This verification link is invalid or has expired.");
        render(&state, "portal-login.html", &ctx).into_response()
    }
}

pub async fn login_form(
    State(state): State<Arc<AppState>>,
    Query(q): Query<LoginQuery>,
) -> impl IntoResponse {
    let mut ctx = tera::Context::new();
    if q.verified.as_deref() == Some("1") {
        ctx.insert("verified", &true);
    }
    if q.reset.as_deref() == Some("1") {
        ctx.insert("reset", &true);
    }
    if q.email_updated.as_deref() == Some("1") {
        ctx.insert("email_updated", &true);
    }
    render(&state, "portal-login.html", &ctx)
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let account: Option<PortalAccount> = sqlx::query_as("SELECT * FROM portal_accounts WHERE email = ?")
        .bind(&form.email)
        .fetch_optional(&*state.forum_db)
        .await
        .unwrap_or(None);

    let account = match account {
        Some(a) => a,
        None => {
            let mut ctx = tera::Context::new();
            ctx.insert("error", "Invalid email or password.");
            return render(&state, "portal-login.html", &ctx);
        }
    };

    let password_ok = verify(&form.password, &account.password_hash).unwrap_or(false);
    if !password_ok {
        let mut ctx = tera::Context::new();
        ctx.insert("error", "Invalid email or password.");
        return render(&state, "portal-login.html", &ctx);
    }

    if !account.is_verified {
        let mut ctx = tera::Context::new();
        ctx.insert(
            "error",
            "Please verify your email before logging in — check your inbox for the verification link.",
        );
        return render(&state, "portal-login.html", &ctx);
    }

    if account.status == "suspended" {
        let mut ctx = tera::Context::new();
        ctx.insert("error", "This account has been suspended. Contact your administrator.");
        return render(&state, "portal-login.html", &ctx);
    }

    let token = create_session(&state.forum_db, &account.id).await;
    let cookie = session_cookie_builder((PORTAL_SESSION_COOKIE, token), &state);
    (jar.add(cookie), Redirect::to("/portal/dashboard")).into_response()
}

pub async fn logout(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Some(cookie) = jar.get(PORTAL_SESSION_COOKIE) {
        let _ = sqlx::query("DELETE FROM portal_sessions WHERE token = ?")
            .bind(cookie.value())
            .execute(&*state.forum_db)
            .await;
    }
    let cookie = session_cookie_builder(PORTAL_SESSION_COOKIE, &state);
    (jar.remove(cookie), Redirect::to("/portal/login"))
}

pub async fn forgot_password_form(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let ctx = tera::Context::new();
    render(&state, "portal-forgot-password.html", &ctx)
}

pub async fn forgot_password(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ForgotPasswordForm>,
) -> impl IntoResponse {
    let account: Option<PortalAccount> = sqlx::query_as("SELECT * FROM portal_accounts WHERE email = ?")
        .bind(&form.email)
        .fetch_optional(&*state.forum_db)
        .await
        .unwrap_or(None);

    // Always show the same "check your email" response whether or not the
    // account exists, so this form can't be used to enumerate registered
    // hospital email addresses.
    if let Some(account) = account {
        let token = Uuid::new_v4().to_string();
        let now = Utc::now();
        let expires = now + Duration::hours(1);
        let _ = sqlx::query(
            "INSERT INTO portal_password_resets (token, account_id, created_at, expires_at, used)
             VALUES (?, ?, ?, ?, 0)"
        )
        .bind(&token)
        .bind(&account.id)
        .bind(now.to_rfc3339())
        .bind(expires.to_rfc3339())
        .execute(&*state.forum_db)
        .await;

        if let Err(e) = send_password_reset_email(&state.config, &account.email, &token).await {
            eprintln!("Failed to send portal password reset email to {}: {}", account.email, e);
        }
    }

    let mut ctx = tera::Context::new();
    ctx.insert("email", &form.email);
    render(&state, "portal-forgot-password-sent.html", &ctx)
}

pub async fn reset_password_form(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ResetPasswordQuery>,
) -> impl IntoResponse {
    let valid = valid_reset_token(&state, &query.token).await;
    let mut ctx = tera::Context::new();
    ctx.insert("token", &query.token);
    if !valid {
        ctx.insert("invalid", &true);
    }
    render(&state, "portal-reset-password.html", &ctx)
}

pub async fn reset_password(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ResetPasswordForm>,
) -> impl IntoResponse {
    let mut ctx = tera::Context::new();
    ctx.insert("token", &form.token);

    let account_id = match reset_token_account_id(&state, &form.token).await {
        Some(id) => id,
        None => {
            ctx.insert("invalid", &true);
            return render(&state, "portal-reset-password.html", &ctx);
        }
    };

    if form.password != form.confirm_password {
        ctx.insert("error", "Passwords do not match.");
        return render(&state, "portal-reset-password.html", &ctx);
    }
    if form.password.len() < 8 {
        ctx.insert("error", "Password must be at least 8 characters.");
        return render(&state, "portal-reset-password.html", &ctx);
    }

    let hashed = match hash(&form.password, DEFAULT_COST) {
        Ok(h) => h,
        Err(_) => {
            ctx.insert("error", "Failed to process password. Please try again.");
            return render(&state, "portal-reset-password.html", &ctx);
        }
    };

    let _ = sqlx::query("UPDATE portal_accounts SET password_hash = ? WHERE id = ?")
        .bind(&hashed)
        .bind(&account_id)
        .execute(&*state.forum_db)
        .await;
    let _ = sqlx::query("UPDATE portal_password_resets SET used = 1 WHERE token = ?")
        .bind(&form.token)
        .execute(&*state.forum_db)
        .await;
    // Resetting the password invalidates any sessions started with the old
    // one, in case the reset was prompted by a compromised credential.
    let _ = sqlx::query("DELETE FROM portal_sessions WHERE account_id = ?")
        .bind(&account_id)
        .execute(&*state.forum_db)
        .await;

    Redirect::to("/portal/login?reset=1").into_response()
}

async fn valid_reset_token(state: &AppState, token: &str) -> bool {
    reset_token_account_id(state, token).await.is_some()
}

async fn reset_token_account_id(state: &AppState, token: &str) -> Option<String> {
    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT account_id, expires_at, used FROM portal_password_resets WHERE token = ?"
    )
    .bind(token)
    .fetch_optional(&*state.forum_db)
    .await
    .ok()?;
    let (account_id, expires_str, used) = row?;
    if used != 0 {
        return None;
    }
    let expires = chrono::DateTime::parse_from_rfc3339(&expires_str).ok()?;
    if expires < Utc::now() {
        return None;
    }
    Some(account_id)
}

pub async fn dashboard(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    let account = match get_current_account(&state, &jar).await {
        Some(a) => a,
        None => return Redirect::to("/portal/login").into_response(),
    };

    let docs: Vec<PortalDocument> = sqlx::query_as(
        "SELECT * FROM portal_documents WHERE account_id = ? ORDER BY uploaded_at DESC"
    )
    .bind(&account.id)
    .fetch_all(&*state.forum_db)
    .await
    .unwrap_or_default();

    let doc_views: Vec<DocumentView> = docs
        .into_iter()
        .map(|d| DocumentView {
            id: d.id,
            original_filename: d.original_filename,
            size_display: format_size(d.size),
            uploaded_at: d.uploaded_at,
        })
        .collect();

    let mut ctx = tera::Context::new();
    ctx.insert("organization", &account.organization);
    ctx.insert("email", &account.email);
    ctx.insert("documents", &doc_views);
    ctx.insert("npi", &account.npi);

    if let Some(npi) = account.npi.as_deref().filter(|n| !n.is_empty()) {
        match crate::npi::lookup(npi).await {
            Ok(Some(info)) => ctx.insert("npi_info", &info),
            // Reachable registry, no matching record — distinct from a
            // lookup failure so the template can word these differently.
            Ok(None) => ctx.insert("npi_not_found", &true),
            Err(_) => ctx.insert("npi_lookup_error", &true),
        }
    }

    render(&state, "portal-dashboard.html", &ctx).into_response()
}

pub async fn upload_document(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let account = match get_current_account(&state, &jar).await {
        Some(a) => a,
        None => return (StatusCode::UNAUTHORIZED, "Login required").into_response(),
    };
    // Defense in depth: the login flow already refuses to create a session for
    // an unverified account, but re-check here rather than trust that alone.
    if !account.is_verified {
        return (
            StatusCode::FORBIDDEN,
            "Please verify your email before uploading documents.",
        )
            .into_response();
    }

    let account_dir = FsPath::new("portal_uploads").join(&account.id);
    if fs::create_dir_all(&account_dir).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to prepare storage").into_response();
    }

    let quota = state
        .config
        .portal_storage_limit_per_account
        .unwrap_or(DEFAULT_PORTAL_ACCOUNT_QUOTA);

    while let Ok(Some(mut field)) = multipart.next_field().await {
        let original_filename = match field.file_name() {
            Some(name) => sanitize_filename::sanitize(name),
            None => continue,
        };
        if original_filename.is_empty() {
            continue;
        }

        let mut bytes: Vec<u8> = Vec::new();
        while let Ok(Some(chunk)) = field.chunk().await {
            bytes.extend_from_slice(&chunk);
        }

        let current_size = crate::upload::get_directory_size(&account_dir).await;
        if current_size + bytes.len() as u64 > quota {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Storage quota exceeded for this account.",
            )
                .into_response();
        }

        // Never trust the client filename as a path — a random prefix per
        // upload avoids both collisions and any traversal tricks that might
        // have slipped past sanitize_filename.
        let stored_name = format!("{}_{}", Uuid::new_v4(), original_filename);
        let stored_path = account_dir.join(&stored_name);
        if fs::write(&stored_path, &bytes).await.is_err() {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to save file").into_response();
        }

        let doc_id = Uuid::new_v4().to_string();
        let _ = sqlx::query(
            "INSERT INTO portal_documents (id, account_id, original_filename, stored_path, size, uploaded_at)
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(&doc_id)
        .bind(&account.id)
        .bind(&original_filename)
        .bind(stored_path.to_string_lossy().to_string())
        .bind(bytes.len() as i64)
        .bind(Utc::now().to_rfc3339())
        .execute(&*state.forum_db)
        .await;
    }

    Redirect::to("/portal/dashboard").into_response()
}

pub async fn download_document(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    AxPath(doc_id): AxPath<String>,
) -> impl IntoResponse {
    let account = match get_current_account(&state, &jar).await {
        Some(a) => a,
        None => return Redirect::to("/portal/login").into_response(),
    };

    let doc: Option<PortalDocument> = sqlx::query_as("SELECT * FROM portal_documents WHERE id = ?")
        .bind(&doc_id)
        .fetch_optional(&*state.forum_db)
        .await
        .unwrap_or(None);

    // The one authorization check that actually matters here: a hospital must
    // never be able to fetch another hospital's document by guessing an id.
    let doc = match doc {
        Some(d) if d.account_id == account.id => d,
        _ => return (StatusCode::NOT_FOUND, "Not found").into_response(),
    };

    let bytes = match fs::read(&doc.stored_path).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::NOT_FOUND, "File missing").into_response(),
    };

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/octet-stream".parse().unwrap());
    let disposition = format!(
        "attachment; filename=\"{}\"",
        doc.original_filename.replace('"', "'")
    );
    if let Ok(value) = disposition.parse() {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }

    (headers, bytes).into_response()
}

fn profile_context(account: &PortalAccount) -> tera::Context {
    let mut ctx = tera::Context::new();
    ctx.insert("organization", &account.organization);
    ctx.insert("email", &account.email);
    ctx.insert("npi", &account.npi);
    ctx
}

pub async fn profile_form(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(q): Query<ProfileQuery>,
) -> impl IntoResponse {
    let account = match get_current_account(&state, &jar).await {
        Some(a) => a,
        None => return Redirect::to("/portal/login").into_response(),
    };

    let mut ctx = profile_context(&account);
    if q.updated.as_deref() == Some("1") {
        ctx.insert("updated", &true);
    }
    if q.password_changed.as_deref() == Some("1") {
        ctx.insert("password_changed", &true);
    }
    if q.email_requested.as_deref() == Some("1") {
        ctx.insert("email_requested", &true);
    }
    if q.email_updated.as_deref() == Some("1") {
        ctx.insert("email_updated", &true);
    }
    render(&state, "portal-profile.html", &ctx).into_response()
}

pub async fn update_profile(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(form): Form<UpdateProfileForm>,
) -> impl IntoResponse {
    let account = match get_current_account(&state, &jar).await {
        Some(a) => a,
        None => return Redirect::to("/portal/login").into_response(),
    };

    let npi_input = form.npi.as_deref().unwrap_or("").trim().to_string();
    let organization = form.organization.trim().to_string();

    let mut ctx = profile_context(&account);
    ctx.insert("organization", &organization);
    ctx.insert("npi", &npi_input);

    if organization.is_empty() {
        ctx.insert("error", "Organization / hospital name is required.");
        return render(&state, "portal-profile.html", &ctx).into_response();
    }
    if !npi_input.is_empty() && !crate::npi::is_valid_format(&npi_input) {
        ctx.insert("error", "NPI number must be exactly 10 digits.");
        return render(&state, "portal-profile.html", &ctx).into_response();
    }

    let npi_to_store = if npi_input.is_empty() { None } else { Some(npi_input.clone()) };
    let _ = sqlx::query("UPDATE portal_accounts SET organization = ?, npi = ? WHERE id = ?")
        .bind(&organization)
        .bind(&npi_to_store)
        .bind(&account.id)
        .execute(&*state.forum_db)
        .await;

    Redirect::to("/portal/profile?updated=1").into_response()
}

pub async fn change_password(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(form): Form<ChangePasswordForm>,
) -> impl IntoResponse {
    let account = match get_current_account(&state, &jar).await {
        Some(a) => a,
        None => return Redirect::to("/portal/login").into_response(),
    };

    let mut ctx = profile_context(&account);

    if !verify(&form.current_password, &account.password_hash).unwrap_or(false) {
        ctx.insert("error", "Current password is incorrect.");
        return render(&state, "portal-profile.html", &ctx).into_response();
    }
    if form.new_password != form.confirm_new_password {
        ctx.insert("error", "New passwords do not match.");
        return render(&state, "portal-profile.html", &ctx).into_response();
    }
    if form.new_password.len() < 8 {
        ctx.insert("error", "New password must be at least 8 characters.");
        return render(&state, "portal-profile.html", &ctx).into_response();
    }

    let hashed = match hash(&form.new_password, DEFAULT_COST) {
        Ok(h) => h,
        Err(_) => {
            ctx.insert("error", "Failed to process password. Please try again.");
            return render(&state, "portal-profile.html", &ctx).into_response();
        }
    };

    let _ = sqlx::query("UPDATE portal_accounts SET password_hash = ? WHERE id = ?")
        .bind(&hashed)
        .bind(&account.id)
        .execute(&*state.forum_db)
        .await;

    // Changing the password invalidates every existing session, including this
    // one, in case the change was prompted by a compromised credential — then
    // immediately issues a fresh session so the user isn't logged out here.
    let _ = sqlx::query("DELETE FROM portal_sessions WHERE account_id = ?")
        .bind(&account.id)
        .execute(&*state.forum_db)
        .await;
    let token = create_session(&state.forum_db, &account.id).await;
    let cookie = session_cookie_builder((PORTAL_SESSION_COOKIE, token), &state);

    (jar.add(cookie), Redirect::to("/portal/profile?password_changed=1")).into_response()
}

pub async fn request_email_change(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(form): Form<ChangeEmailForm>,
) -> impl IntoResponse {
    let account = match get_current_account(&state, &jar).await {
        Some(a) => a,
        None => return Redirect::to("/portal/login").into_response(),
    };

    let new_email = form.new_email.trim().to_string();
    let mut ctx = profile_context(&account);

    if !verify(&form.current_password, &account.password_hash).unwrap_or(false) {
        ctx.insert("error", "Current password is incorrect.");
        return render(&state, "portal-profile.html", &ctx).into_response();
    }
    if new_email.is_empty() || !new_email.contains('@') {
        ctx.insert("error", "Enter a valid email address.");
        return render(&state, "portal-profile.html", &ctx).into_response();
    }
    if new_email.eq_ignore_ascii_case(&account.email) {
        ctx.insert("error", "That's already the email on file.");
        return render(&state, "portal-profile.html", &ctx).into_response();
    }

    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM portal_accounts WHERE email = ?")
        .bind(&new_email)
        .fetch_one(&*state.forum_db)
        .await
        .unwrap_or(0);
    if existing > 0 {
        ctx.insert("error", "An account with this email already exists.");
        return render(&state, "portal-profile.html", &ctx).into_response();
    }

    let token = Uuid::new_v4().to_string();
    let now = Utc::now();
    let expires = now + Duration::hours(1);
    let _ = sqlx::query(
        "INSERT INTO portal_email_changes (token, account_id, new_email, created_at, expires_at, used)
         VALUES (?, ?, ?, ?, ?, 0)"
    )
    .bind(&token)
    .bind(&account.id)
    .bind(&new_email)
    .bind(now.to_rfc3339())
    .bind(expires.to_rfc3339())
    .execute(&*state.forum_db)
    .await;

    if let Err(e) = send_email_change_email(&state.config, &new_email, &token).await {
        eprintln!("Failed to send portal email-change email to {}: {}", new_email, e);
    }

    Redirect::to("/portal/profile?email_requested=1").into_response()
}

pub async fn confirm_email_change(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(query): Query<ConfirmEmailQuery>,
) -> impl IntoResponse {
    let row: Option<(String, String, String, i64)> = sqlx::query_as(
        "SELECT account_id, new_email, expires_at, used FROM portal_email_changes WHERE token = ?"
    )
    .bind(&query.token)
    .fetch_optional(&*state.forum_db)
    .await
    .unwrap_or(None);

    let (account_id, new_email) = match row {
        Some((account_id, new_email, expires_str, used)) => {
            let expired = used != 0
                || chrono::DateTime::parse_from_rfc3339(&expires_str)
                    .map(|e| e < Utc::now())
                    .unwrap_or(true);
            if expired {
                return (StatusCode::BAD_REQUEST, "This email confirmation link is invalid or has expired.").into_response();
            }
            (account_id, new_email)
        }
        None => {
            return (StatusCode::BAD_REQUEST, "This email confirmation link is invalid or has expired.").into_response();
        }
    };

    // Re-check uniqueness at confirmation time too, in case the address was
    // claimed by another account in the window between request and click.
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM portal_accounts WHERE email = ? AND id != ?")
        .bind(&new_email)
        .bind(&account_id)
        .fetch_one(&*state.forum_db)
        .await
        .unwrap_or(0);
    if existing > 0 {
        return (StatusCode::CONFLICT, "That email address is now in use by another account.").into_response();
    }

    let _ = sqlx::query("UPDATE portal_accounts SET email = ? WHERE id = ?")
        .bind(&new_email)
        .bind(&account_id)
        .execute(&*state.forum_db)
        .await;
    let _ = sqlx::query("UPDATE portal_email_changes SET used = 1 WHERE token = ?")
        .bind(&query.token)
        .execute(&*state.forum_db)
        .await;

    match get_current_account(&state, &jar).await {
        Some(current) if current.id == account_id => {
            Redirect::to("/portal/profile?email_updated=1").into_response()
        }
        _ => Redirect::to("/portal/login?email_updated=1").into_response(),
    }
}
