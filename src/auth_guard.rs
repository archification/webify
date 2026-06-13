use axum::{
    extract::{State, Query},
    response::{Html, IntoResponse, Redirect},
    http::StatusCode,
};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use crate::AppState;
use crate::forum::{ForumDb, reqwest_async_http_client};
use crate::config::AuthGuard;
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken,
    RedirectUrl, Scope, TokenResponse, TokenUrl, AuthorizationCode,
};
use reqwest::Client as ReqwestClient;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;
use chrono::{Utc, Duration};
use urlencoding::encode as urlencode;

pub const GUARD_COOKIE: &str = "guard_token";

#[derive(Deserialize)]
pub struct LoginQuery {
    pub next: Option<String>,
    pub host: Option<String>,
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: Option<String>,
}

/// Find the first auth guard that matches both the hostname and request path.
/// A guard with an empty `sites` list applies to all domains.
pub fn find_guard<'a>(guards: &'a [AuthGuard], host: &str, path: &str) -> Option<&'a AuthGuard> {
    guards.iter().find(|g| {
        let site_match = g.sites.is_empty()
            || g.sites.iter().any(|s| s.eq_ignore_ascii_case(host));
        let path_match = g.paths.iter().any(|p| {
            let prefix = p.trim_end_matches('/');
            path == prefix || path.starts_with(&format!("{}/", prefix))
        });
        site_match && path_match
    })
}

/// Check whether an email address is permitted by a guard rule.
pub fn email_allowed(guard: &AuthGuard, email: &str) -> bool {
    let email_lc = email.to_ascii_lowercase();
    if guard.allowed_emails.iter().any(|e| e.to_ascii_lowercase() == email_lc) {
        return true;
    }
    if let Some(domain) = email_lc.split('@').nth(1) {
        if guard.allowed_domains.iter().any(|d| d.to_ascii_lowercase() == domain) {
            return true;
        }
    }
    false
}

#[derive(Clone)]
pub struct AccessRule {
    pub domain: String,
    pub path: String,
    pub email: Option<String>,
    pub email_domain: Option<String>,
}

pub async fn load_access_rules(db: &ForumDb) -> Vec<AccessRule> {
    let rows: Vec<(String, String, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT domain, path, email, email_domain FROM access_rules")
            .fetch_all(&**db)
            .await
            .unwrap_or_default();
    rows.into_iter()
        .map(|(domain, path, email, email_domain)| AccessRule { domain, path, email, email_domain })
        .collect()
}

pub fn has_db_rule(rules: &[AccessRule], host: &str, path: &str) -> bool {
    rules.iter().any(|rule| {
        let site_match = rule.domain.is_empty() || rule.domain.eq_ignore_ascii_case(host);
        let prefix = rule.path.trim_end_matches('/');
        let path_match = path == prefix || path.starts_with(&format!("{}/", prefix));
        site_match && path_match
    })
}

pub fn db_rule_allows(rules: &[AccessRule], host: &str, path: &str, email: &str) -> bool {
    let email_lc = email.to_ascii_lowercase();
    let user_domain = email_lc.split('@').nth(1).unwrap_or("");
    rules.iter().any(|rule| {
        let site_match = rule.domain.is_empty() || rule.domain.eq_ignore_ascii_case(host);
        let prefix = rule.path.trim_end_matches('/');
        let path_match = path == prefix || path.starts_with(&format!("{}/", prefix));
        if !site_match || !path_match {
            return false;
        }
        if let Some(ref re) = rule.email {
            if re.to_ascii_lowercase() == email_lc {
                return true;
            }
        }
        if let Some(ref red) = rule.email_domain {
            if red.to_ascii_lowercase() == user_domain {
                return true;
            }
        }
        false
    })
}

/// Extract a named cookie value from a raw `Cookie:` header string.
pub fn extract_cookie_value(header: &str, name: &str) -> Option<String> {
    for pair in header.split(';') {
        let pair = pair.trim();
        if let Some((k, v)) = pair.split_once('=') {
            if k.trim() == name {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Look up a guard session token and return the associated email if valid and unexpired.
pub async fn validate_session(db: &ForumDb, token: &str) -> Option<String> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT email, expires_at FROM guard_sessions WHERE token = ?")
            .bind(token)
            .fetch_optional(&**db)
            .await
            .ok()?;
    let (email, expires_str) = row?;
    let expires = chrono::DateTime::parse_from_rfc3339(&expires_str).ok()?;
    if expires < Utc::now() {
        let _ = sqlx::query("DELETE FROM guard_sessions WHERE token = ?")
            .bind(token)
            .execute(&**db)
            .await;
        return None;
    }
    Some(email)
}

/// Create a new guard session for an email; returns the session token.
pub async fn create_session(db: &ForumDb, email: &str) -> String {
    let token = Uuid::new_v4().to_string();
    let now = Utc::now();
    let expires = now + Duration::days(7);
    let _ = sqlx::query(
        "INSERT INTO guard_sessions (token, email, created_at, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&token)
    .bind(email)
    .bind(now.to_rfc3339())
    .bind(expires.to_rfc3339())
    .execute(&**db)
    .await;
    token
}

fn safe_next(raw: Option<String>) -> String {
    match raw {
        Some(s) if s.starts_with('/') && !s.starts_with("//") => s,
        _ => "/".to_string(),
    }
}

/// Encode host + path into a single OAuth state string.
fn encode_state(host: &str, path: &str) -> String {
    format!("{}|{}", host, path)
}

/// Decode the OAuth state string back into (host, path).
fn decode_state(state: &str) -> (&str, &str) {
    if let Some((h, p)) = state.split_once('|') {
        if p.starts_with('/') && !p.starts_with("//") {
            return (h, p);
        }
    }
    ("", "/")
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// GET /auth/login?next=/protected/path&host=example.com — branded sign-in page
pub async fn guard_login(
    State(state): State<Arc<AppState>>,
    Query(q): Query<LoginQuery>,
) -> impl IntoResponse {
    let next = safe_next(q.next);
    let host = q.host.unwrap_or_default();
    let configured = state.config.google_client_id.is_some()
        && state.config.google_client_secret.is_some()
        && state.config.guard_redirect_url.is_some();

    // Use a custom login page from the filesystem if configured.
    if configured {
        if let Some(ref path) = state.config.guard_login_page {
            if let Ok(template) = std::fs::read_to_string(path) {
                let body = template
                    .replace("{next}", &urlencode(&next))
                    .replace("{host}", &urlencode(&host));
                return Html(body);
            }
        }
    }

    let body = if configured {
        format!(
            r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Sign In — Capital Pulse</title>
  <style>
    :root {{
      --deep:#0d1719; --surface-dark:#15282b; --white:#f5f7f7;
      --text-muted:#8fa6a9; --teal:#5db6bf; --radius:16px; --radius-sm:8px;
      --font-display:'DM Serif Display',Georgia,serif;
      --font-body:'Poppins',system-ui,sans-serif;
    }}
    body {{ font-family:var(--font-body); color:var(--white); margin:0; }}
  </style>
  <link href="https://fonts.googleapis.com/css2?family=DM+Serif+Display:ital@0;1&family=Poppins:wght@300;400;600;700&display=swap" rel="stylesheet">
  <style>
    body {{ background: var(--deep); }}
    .auth-wrap {{ min-height: 100vh; display: flex; align-items: center; justify-content: center; }}
    .auth-card {{
      background: var(--surface-dark);
      border: 1px solid rgba(93,182,191,0.18);
      border-radius: var(--radius);
      padding: 48px 40px;
      max-width: 400px;
      width: 90%;
      text-align: center;
    }}
    .auth-logo {{ margin: 0 auto 32px; max-width: 180px; display: block; }}
    .auth-title {{ font-family: var(--font-display); font-size: 1.6rem; font-weight: 400; color: var(--white); margin-bottom: 8px; }}
    .auth-sub {{ font-size: 0.875rem; color: var(--text-muted); margin-bottom: 36px; line-height: 1.6; }}
    .btn-google {{
      display: inline-flex; align-items: center; gap: 12px;
      background: #fff; color: #3c4043;
      font-family: var(--font-body); font-size: 0.875rem; font-weight: 600;
      padding: 13px 24px; border-radius: var(--radius-sm); border: none;
      cursor: pointer; text-decoration: none;
      transition: box-shadow 0.2s;
      width: 100%; justify-content: center;
    }}
    .btn-google:hover {{ box-shadow: 0 2px 14px rgba(0,0,0,0.3); }}
  </style>
</head>
<body>
  <div class="auth-wrap">
    <div class="auth-card">
      <div class="auth-logo" style="display:flex;align-items:center;justify-content:center;gap:12px;max-width:none;">
        <svg width="36" height="36" viewBox="0 0 48 48" fill="none" xmlns="http://www.w3.org/2000/svg">
          <circle cx="24" cy="24" r="21" stroke="#5db6bf" stroke-width="2.5"/>
          <path d="M8 24h7l4-11 6 22 4-13 3 2h8" stroke="#5db6bf" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
        <span style="font-family:'DM Serif Display',Georgia,serif;font-size:1.5rem;color:#f5f7f7;">Capital Pulse</span>
      </div>
      <h1 class="auth-title">Sign In</h1>
      <p class="auth-sub">This page is restricted. Sign in with your authorized Google account to continue.</p>
      <a href="/auth/google?next={next}&host={host}" class="btn-google">
        <svg width="18" height="18" viewBox="0 0 18 18" xmlns="http://www.w3.org/2000/svg">
          <path d="M17.64 9.2c0-.637-.057-1.251-.164-1.84H9v3.481h4.844c-.209 1.125-.843 2.078-1.796 2.717v2.258h2.908C16.658 13.652 17.64 11.345 17.64 9.2z" fill="#4285F4"/>
          <path d="M9 18c2.43 0 4.467-.806 5.956-2.18l-2.908-2.259c-.806.54-1.837.86-3.048.86-2.344 0-4.328-1.584-5.036-3.711H.957v2.332A8.997 8.997 0 0 0 9 18z" fill="#34A853"/>
          <path d="M3.964 10.71A5.41 5.41 0 0 1 3.682 9c0-.593.102-1.17.282-1.71V4.958H.957A8.996 8.996 0 0 0 0 9c0 1.452.348 2.827.957 4.042l3.007-2.332z" fill="#FBBC05"/>
          <path d="M9 3.58c1.321 0 2.508.454 3.44 1.345l2.582-2.58C13.463.891 11.426 0 9 0A8.997 8.997 0 0 0 .957 4.958L3.964 6.29C4.672 4.163 6.656 3.58 9 3.58z" fill="#EA4335"/>
        </svg>
        Continue with Google
      </a>
    </div>
  </div>
</body>
</html>"##,
            next = urlencode(&next),
            host = urlencode(&host)
        )
    } else {
        r#"<!DOCTYPE html><html><body style="font-family:sans-serif;padding:80px;text-align:center">
<h1>Auth not configured</h1>
<p>Set <code>google_client_id</code>, <code>google_client_secret</code>, and <code>guard_redirect_url</code> in config.toml.</p>
</body></html>"#.to_string()
    };

    Html(body)
}

/// GET /auth/google?next=/protected/path&host=example.com — redirect to Google OAuth
pub async fn guard_google(
    State(state): State<Arc<AppState>>,
    Query(q): Query<LoginQuery>,
) -> impl IntoResponse {
    let next = safe_next(q.next);
    let host = q.host.unwrap_or_default();
    let config = &state.config;
    let (id, secret, redirect_url) = match (
        &config.google_client_id,
        &config.google_client_secret,
        &config.guard_redirect_url,
    ) {
        (Some(i), Some(s), Some(r)) => (i, s, r),
        _ => return (StatusCode::INTERNAL_SERVER_ERROR, "Auth not configured").into_response(),
    };
    let client = BasicClient::new(ClientId::new(id.clone()))
        .set_client_secret(ClientSecret::new(secret.clone()))
        .set_auth_uri(
            AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string()).unwrap(),
        )
        .set_token_uri(
            TokenUrl::new("https://oauth2.googleapis.com/token".to_string()).unwrap(),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_url.clone()).unwrap());

    // Encode host + path into the OAuth state so the callback can reconstruct both.
    let (auth_url, _) = client
        .authorize_url(|| CsrfToken::new(encode_state(&host, &next)))
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .add_extra_param("prompt", "select_account")
        .url();

    Redirect::to(auth_url.as_str()).into_response()
}

/// GET /auth/callback?code=...&state=... — Google OAuth callback
pub async fn guard_callback(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(q): Query<CallbackQuery>,
) -> impl IntoResponse {
    let raw_state = q.state.unwrap_or_default();
    let (cb_host, next) = decode_state(&raw_state);
    let cb_host = cb_host.to_string();
    let next = next.to_string();
    let config = &state.config;
    let (id, secret, redirect_url) = match (
        &config.google_client_id,
        &config.google_client_secret,
        &config.guard_redirect_url,
    ) {
        (Some(i), Some(s), Some(r)) => (i, s, r),
        _ => return (StatusCode::INTERNAL_SERVER_ERROR, "Auth not configured").into_response(),
    };
    let client = BasicClient::new(ClientId::new(id.clone()))
        .set_client_secret(ClientSecret::new(secret.clone()))
        .set_auth_uri(
            AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string()).unwrap(),
        )
        .set_token_uri(
            TokenUrl::new("https://oauth2.googleapis.com/token".to_string()).unwrap(),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_url.clone()).unwrap());

    let token = match client
        .exchange_code(AuthorizationCode::new(q.code))
        .request_async(&reqwest_async_http_client)
        .await
    {
        Ok(t) => t,
        Err(_) => return (StatusCode::BAD_REQUEST, "Failed to exchange token").into_response(),
    };

    #[derive(Deserialize)]
    struct GoogleUser {
        email: String,
    }
    let resp = match ReqwestClient::new()
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(token.access_token().secret())
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch user info").into_response(),
    };
    let user_info: GoogleUser = match resp.json::<GoogleUser>().await {
        Ok(u) => u,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to parse user info").into_response(),
    };

    // Check whether this email is permitted for the requested destination.
    if let Some(guard) = find_guard(&config.auth_guards, &cb_host, &next) {
        if !email_allowed(guard, &user_info.email) {
            return Html(format!(
                r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Access Denied — Capital Pulse</title>
  <style>
    :root {{
      --deep:#0d1719; --surface-dark:#15282b; --white:#f5f7f7;
      --text-muted:#8fa6a9; --teal:#5db6bf; --radius:16px; --radius-sm:8px;
      --font-display:'DM Serif Display',Georgia,serif;
      --font-body:'Poppins',system-ui,sans-serif;
    }}
    body {{ font-family:var(--font-body); color:var(--white); margin:0; }}
  </style>
  <link href="https://fonts.googleapis.com/css2?family=DM+Serif+Display:ital@0;1&family=Poppins:wght@300;400;600;700&display=swap" rel="stylesheet">
  <style>
    body {{ background: var(--deep); }}
    .auth-wrap {{ min-height: 100vh; display: flex; align-items: center; justify-content: center; }}
    .auth-card {{ background: var(--surface-dark); border: 1px solid rgba(220,50,47,0.3); border-radius: var(--radius); padding: 48px 40px; max-width: 420px; width: 90%; text-align: center; }}
    .auth-logo {{ margin: 0 auto 32px; max-width: 180px; display: block; }}
    .auth-title {{ font-family: var(--font-display); font-size: 1.5rem; font-weight: 400; color: var(--white); margin-bottom: 12px; }}
    .auth-email {{ font-size: 0.875rem; color: var(--teal); margin-bottom: 8px; }}
    .auth-sub {{ font-size: 0.875rem; color: var(--text-muted); margin-bottom: 32px; line-height: 1.6; }}
    .btn-retry {{ display: inline-flex; align-items: center; justify-content: center; padding: 12px 28px; background: transparent; border: 2px solid var(--teal); color: var(--teal); border-radius: var(--radius-sm); font-family: var(--font-body); font-size: 0.8rem; font-weight: 700; letter-spacing: 0.08em; text-transform: uppercase; text-decoration: none; transition: background 0.2s, color 0.2s; }}
    .btn-retry:hover {{ background: var(--teal); color: var(--deep); }}
  </style>
</head>
<body>
  <div class="auth-wrap">
    <div class="auth-card">
      <div class="auth-logo" style="display:flex;align-items:center;justify-content:center;gap:12px;max-width:none;">
        <svg width="36" height="36" viewBox="0 0 48 48" fill="none" xmlns="http://www.w3.org/2000/svg">
          <circle cx="24" cy="24" r="21" stroke="#5db6bf" stroke-width="2.5"/>
          <path d="M8 24h7l4-11 6 22 4-13 3 2h8" stroke="#5db6bf" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
        <span style="font-family:'DM Serif Display',Georgia,serif;font-size:1.5rem;color:#f5f7f7;">Capital Pulse</span>
      </div>
      <h1 class="auth-title">Access Denied</h1>
      <p class="auth-email">{email}</p>
      <p class="auth-sub">This account is not authorized to access this page. Please contact your administrator or try a different account.</p>
      <a href="/auth/login?next={next_enc}" class="btn-retry">Try a different account</a>
    </div>
  </div>
</body>
</html>"##,
                email = user_info.email,
                next_enc = urlencode(&next),
            ))
            .into_response();
        }
    }

    let session_token = create_session(&state.forum_db, &user_info.email).await;
    let cookie = Cookie::build((GUARD_COOKIE, session_token))
        .path("/")
        .http_only(true)
        .build();
    (jar.add(cookie), Redirect::to(&next)).into_response()
}

/// GET /auth/logout — clear the guard session cookie
pub async fn guard_logout(jar: CookieJar) -> impl IntoResponse {
    let cookie = Cookie::build(GUARD_COOKIE).path("/").build();
    (jar.remove(cookie), Redirect::to("/"))
}
