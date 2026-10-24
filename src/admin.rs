use axum::{
    extract::{State, Form, Query},
    response::{Html, IntoResponse, Redirect},
    http::{HeaderMap, StatusCode, header},
};
use axum_extra::extract::CookieJar;
use crate::AppState;
use crate::auth_guard;
use crate::config::AdminDashboard;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use chrono::Utc;
use uuid::Uuid;
use urlencoding::encode as urlencode;
use serde_json;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AccessRuleRow {
    pub id: String,
    pub domain: String,
    pub path: String,
    pub email: Option<String>,
    pub email_domain: Option<String>,
    pub tag_type: Option<String>,
    pub tag_value: Option<String>,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct EditorRow {
    pub email: String,
    pub granted_by: String,
    pub granted_at: String,
}

#[derive(Deserialize)]
pub struct AddRuleForm {
    pub domain: String,
    pub path: String,
    pub email: Option<String>,
    pub email_domain: Option<String>,
    pub tag_type: Option<String>,
    pub tag_value: Option<String>,
}

#[derive(Deserialize)]
pub struct AddEditorForm {
    pub email: String,
}

#[derive(Deserialize)]
pub struct DeleteRuleForm {
    pub id: String,
}

#[derive(Deserialize)]
pub struct RevokeEditorForm {
    pub email: String,
}

#[derive(Deserialize)]
pub struct AddFileGuardForm {
    pub label: String,
    pub paths: String,
    pub hash: String,
}

#[derive(Deserialize)]
pub struct DeleteFileGuardForm {
    pub id: String,
}

#[derive(Deserialize)]
pub struct SetSubscriptionForm {
    pub email: String,
    pub tier: String,
    pub status: String,
    pub expires_at: String,
}

#[derive(Deserialize)]
pub struct GrantWriterForm {
    pub email: String,
}

#[derive(Deserialize)]
pub struct RevokeWriterForm {
    pub email: String,
}

#[derive(Deserialize)]
pub struct PrivateAccountForm {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct CreateAccountForm {
    pub organization: String,
    pub email: String,
    #[serde(default)]
    pub npi: Option<String>,
}

#[derive(Deserialize)]
pub struct RemoveAccountForm {
    pub email: String,
}

#[derive(Deserialize)]
pub struct SuspendAccountForm {
    pub email: String,
}

#[derive(Deserialize)]
pub struct UnsuspendAccountForm {
    pub email: String,
}

#[derive(Deserialize)]
pub struct PresentAccountForm {
    pub username: String,
    #[serde(default)]
    pub display_name: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct PresentPasswordForm {
    pub id: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct PresentAccountIdForm {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub struct AccountRow {
    pub email: String,
    pub display_name: String,
    pub organization: Option<String>,
    pub sources: Vec<String>,
    pub forum_role: Option<String>,
    pub is_writer: bool,
    /// Whether `is_writer` came from a DB access_rules row targeting this
    /// exact email (and can therefore be revoked from the dashboard), as
    /// opposed to a config.toml auth_guard or an email_domain-wide rule.
    pub writer_removable: bool,
    pub is_internal: bool,
    pub sub_status: String,
    pub sub_tier: String,
    pub sub_expires_at: Option<String>,
    /// "active" / "suspended", or None if this email has no portal_accounts row.
    pub portal_status: Option<String>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn find_dashboard_for_host<'a>(dashboards: &'a [AdminDashboard], host: &str) -> Option<&'a AdminDashboard> {
    dashboards.iter().find(|d| {
        d.domain.is_empty() || d.domain.eq_ignore_ascii_case(host)
    })
}

fn get_hostname(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.split(':').next())
        .unwrap_or("")
        .to_string()
}

async fn session_email(state: &Arc<AppState>, jar: &CookieJar) -> Option<String> {
    let token = jar.get(auth_guard::GUARD_COOKIE)?.value().to_string();
    auth_guard::validate_session(&state.forum_db, &token).await
}

fn is_owner(dashboard: &AdminDashboard, email: &str) -> bool {
    let elc = email.to_ascii_lowercase();
    dashboard.owners.iter().any(|o| o.to_ascii_lowercase() == elc)
}

async fn is_editor(state: &Arc<AppState>, email: &str) -> bool {
    let elc = email.to_ascii_lowercase();
    sqlx::query_scalar::<_, bool>("SELECT COUNT(*) > 0 FROM dashboard_editors WHERE email = ?")
        .bind(&elc)
        .fetch_one(&*state.forum_db)
        .await
        .unwrap_or(false)
}

/// Returns (email, is_owner) or an error response if not authorized.
async fn require_dashboard_access(
    state: &Arc<AppState>,
    jar: &CookieJar,
    headers: &HeaderMap,
    dash_path: &str,
) -> Result<(String, bool), axum::response::Response> {
    let hostname = get_hostname(headers);
    let email = match session_email(state, jar).await {
        Some(e) => e,
        None => {
            let url = format!("/auth/login?next={}&host={}", urlencode(dash_path), urlencode(&hostname));
            return Err(Redirect::to(&url).into_response());
        }
    };
    let dashboard = match find_dashboard_for_host(&state.config.admin_dashboards, &hostname) {
        Some(d) => d,
        None => return Err((StatusCode::NOT_FOUND, "Dashboard not configured").into_response()),
    };
    let owner = is_owner(dashboard, &email);
    if !owner && !is_editor(state, &email).await {
        return Err((StatusCode::FORBIDDEN, "Access denied").into_response());
    }
    Ok((email, owner))
}

fn get_or_insert<'a>(rows: &'a mut HashMap<String, AccountRow>, email_raw: &str) -> &'a mut AccountRow {
    let key = email_raw.to_ascii_lowercase();
    rows.entry(key.clone()).or_insert_with(move || AccountRow {
        email: key,
        display_name: String::new(),
        organization: None,
        sources: vec![],
        forum_role: None,
        is_writer: false,
        writer_removable: false,
        is_internal: false,
        sub_status: "none".to_string(),
        sub_tier: String::new(),
        sub_expires_at: None,
        portal_status: None,
    })
}

pub async fn build_account_rows(state: &Arc<AppState>) -> Vec<AccountRow> {
    let mut rows: HashMap<String, AccountRow> = HashMap::new();

    let forum_users: Vec<(String, String, String)> =
        sqlx::query_as("SELECT username, email, role FROM users")
            .fetch_all(&*state.forum_db)
            .await
            .unwrap_or_default();
    for (username, email, role) in forum_users {
        if email.is_empty() { continue; }
        let row = get_or_insert(&mut rows, &email);
        row.display_name = username;
        row.forum_role = Some(role);
        row.sources.push("Forum".to_string());
    }

    let portal_accounts: Vec<(String, String, String)> =
        sqlx::query_as("SELECT email, organization, status FROM portal_accounts")
            .fetch_all(&*state.forum_db)
            .await
            .unwrap_or_default();
    for (email, organization, status) in portal_accounts {
        let row = get_or_insert(&mut rows, &email);
        if row.display_name.is_empty() {
            row.display_name = organization.clone();
        }
        row.organization = Some(organization);
        row.portal_status = Some(status);
        row.sources.push("Portal".to_string());
    }

    let editors: Vec<(String,)> =
        sqlx::query_as("SELECT email FROM dashboard_editors")
            .fetch_all(&*state.forum_db)
            .await
            .unwrap_or_default();
    for (email,) in editors {
        get_or_insert(&mut rows, &email).sources.push("Dashboard Editor".to_string());
    }

    let signed_in: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT email FROM guard_sessions")
            .fetch_all(&*state.forum_db)
            .await
            .unwrap_or_default();
    for (email,) in signed_in {
        get_or_insert(&mut rows, &email).sources.push("Signed In".to_string());
    }

    for dashboard in &state.config.admin_dashboards {
        for owner in &dashboard.owners {
            get_or_insert(&mut rows, owner).sources.push("Dashboard Owner".to_string());
        }
    }

    let today = Utc::now().date_naive();
    let subs: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT email, tier, status, expires_at FROM account_subscriptions",
    )
    .fetch_all(&*state.forum_db)
    .await
    .unwrap_or_default();
    for (email, tier, status, expires_at) in subs {
        let row = get_or_insert(&mut rows, &email);
        row.sub_tier = tier;
        row.sub_status = if status == "active" {
            // expires_at comes from an HTML <input type="date">, so it's a plain
            // YYYY-MM-DD, not a full RFC3339 timestamp.
            let expired = expires_at.as_deref()
                .and_then(|e| chrono::NaiveDate::parse_from_str(e, "%Y-%m-%d").ok())
                .map(|e| e < today)
                .unwrap_or(false);
            if expired { "expired".to_string() } else { status }
        } else {
            status
        };
        row.sub_expires_at = expires_at;
    }

    let access_rules = state.access_rules.read().await.clone();
    for row in rows.values_mut() {
        if row.display_name.is_empty() {
            row.display_name = row.email.clone();
        }
        row.is_writer = auth_guard::email_is_writer(&state.config.auth_guards, &access_rules, &row.email);
        row.is_internal = auth_guard::is_internal_email(&state.config.domain, &row.email);
        let email_lc = row.email.to_ascii_lowercase();
        row.writer_removable = access_rules.iter().any(|r| {
            auth_guard::path_matches_write_route(&r.path)
                && r.email.as_deref().map(|e| e.eq_ignore_ascii_case(&email_lc)).unwrap_or(false)
        });
    }

    let mut out: Vec<AccountRow> = rows.into_values().collect();
    out.sort_by(|a, b| a.email.cmp(&b.email));
    out
}

// ── Handlers ─────────────────────────────────────────────────────────────────

pub async fn dashboard_page(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    dash_path: String,
) -> impl IntoResponse {
    let (email, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };

    let rules: Vec<AccessRuleRow> = sqlx::query_as(
        "SELECT id, domain, path, email, email_domain, tag_type, tag_value, created_by, created_at \
         FROM access_rules ORDER BY created_at DESC",
    )
    .fetch_all(&*state.forum_db)
    .await
    .unwrap_or_default();

    let editors: Vec<EditorRow> = if owner {
        sqlx::query_as(
            "SELECT email, granted_by, granted_at FROM dashboard_editors ORDER BY granted_at DESC",
        )
        .fetch_all(&*state.forum_db)
        .await
        .unwrap_or_default()
    } else {
        vec![]
    };

    let flash = params.get("flash").cloned().unwrap_or_default();
    let flash_error = params.get("flash_error").cloned().unwrap_or_default();

    let db_file_guards = crate::file_gate::load_db_file_guards(&state.forum_db).await;
    let accounts = build_account_rows(&state).await;

    let mut ctx = tera::Context::new();
    ctx.insert("email", &email);
    ctx.insert("is_owner", &owner);
    ctx.insert("rules", &rules);
    ctx.insert("editors", &editors);
    ctx.insert("dashboard_path", &dash_path);
    ctx.insert("flash", &flash);
    ctx.insert("flash_error", &flash_error);
    ctx.insert("config_rules", &state.config.auth_guards);
    ctx.insert("config_file_guards", &state.config.file_guards);
    ctx.insert("db_file_guards", &db_file_guards);
    ctx.insert("accounts", &accounts);
    ctx.insert("present_accounts", &crate::presentation::admin_account_rows(&state.forum_db).await);
    ctx.insert("present_path", &crate::presentation::configured_path(&state.config));
    ctx.insert("private_accounts", &crate::private_media::admin_account_rows(&state.forum_db).await);
    ctx.insert("private_path", &crate::private_media::configured_path(&state.config));

    match state.tera.render("admin_dashboard.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn add_rule(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<AddRuleForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (email, _) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };

    let path = form.path.trim().to_string();
    if path.is_empty() || !path.starts_with('/') {
        let url = format!("{}?flash_error=Path+must+start+with+/", dash_path);
        return Redirect::to(&url).into_response();
    }

    let email_val = form.email.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_lowercase);
    let domain_val = form.email_domain.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_lowercase);
    let tag_type_val = form.tag_type.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_lowercase);

    if let Some(ref t) = tag_type_val {
        if !matches!(t.as_str(), "writer" | "internal" | "external" | "tier") {
            let url = format!("{}?flash_error=Unknown+tag", dash_path);
            return Redirect::to(&url).into_response();
        }
    }

    // The tier name only means anything alongside the "tier" tag — drop it
    // otherwise so a stray value in the always-visible field isn't stored.
    let tag_value_val = if tag_type_val.as_deref() == Some("tier") {
        let v = form.tag_value.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        if v.is_none() {
            let url = format!("{}?flash_error=Specify+a+tier+name+for+the+Subscription+Tier+tag", dash_path);
            return Redirect::to(&url).into_response();
        }
        v
    } else {
        None
    };

    if email_val.is_none() && domain_val.is_none() && tag_type_val.is_none() {
        let url = format!("{}?flash_error=Specify+at+least+one+of+email,+email+domain,+or+tag", dash_path);
        return Redirect::to(&url).into_response();
    }

    let id = Uuid::new_v4().to_string();
    let domain = form.domain.trim().to_lowercase();
    let now = Utc::now().to_rfc3339();

    let _ = sqlx::query(
        "INSERT INTO access_rules (id, domain, path, email, email_domain, tag_type, tag_value, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&domain)
    .bind(&path)
    .bind(&email_val)
    .bind(&domain_val)
    .bind(&tag_type_val)
    .bind(&tag_value_val)
    .bind(&email)
    .bind(&now)
    .execute(&*state.forum_db)
    .await;

    *state.access_rules.write().await = crate::auth_guard::load_access_rules(&state.forum_db).await;

    Redirect::to(&format!("{}?flash=Rule+added", dash_path)).into_response()
}

pub async fn delete_rule(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<DeleteRuleForm>,
    dash_path: String,
) -> impl IntoResponse {
    if let Err(r) = require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        return r;
    }
    let _ = sqlx::query("DELETE FROM access_rules WHERE id = ?")
        .bind(&form.id)
        .execute(&*state.forum_db)
        .await;
    *state.access_rules.write().await = crate::auth_guard::load_access_rules(&state.forum_db).await;
    Redirect::to(&format!("{}?flash=Rule+removed", dash_path)).into_response()
}

pub async fn add_editor(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<AddEditorForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (email, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can manage editors").into_response();
    }

    let new_email = form.email.trim().to_lowercase();
    if new_email.is_empty() || !new_email.contains('@') {
        let url = format!("{}?flash_error=Invalid+email+address", dash_path);
        return Redirect::to(&url).into_response();
    }

    let now = Utc::now().to_rfc3339();
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO dashboard_editors (email, granted_by, granted_at) VALUES (?, ?, ?)",
    )
    .bind(&new_email)
    .bind(&email)
    .bind(&now)
    .execute(&*state.forum_db)
    .await;

    Redirect::to(&format!("{}?flash=Editor+access+granted&tab=editors", dash_path)).into_response()
}

pub async fn revoke_editor(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<RevokeEditorForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (_, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can manage editors").into_response();
    }
    let target = form.email.trim().to_lowercase();
    let _ = sqlx::query("DELETE FROM dashboard_editors WHERE email = ?")
        .bind(&target)
        .execute(&*state.forum_db)
        .await;
    Redirect::to(&format!("{}?flash=Editor+access+revoked&tab=editors", dash_path)).into_response()
}

pub async fn add_file_guard(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<AddFileGuardForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (email, _) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };

    let label = form.label.trim().to_string();
    let hash = form.hash.trim().to_lowercase();
    let paths: Vec<String> = form.paths
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.starts_with('/'))
        .collect();

    if label.is_empty()
        || hash.len() != 64
        || !hash.chars().all(|c| c.is_ascii_hexdigit())
        || paths.is_empty()
    {
        let url = format!("{}?flash_error=Invalid+parameters+check+hash+and+paths&tab=file-gates", dash_path);
        return Redirect::to(&url).into_response();
    }

    let id = Uuid::new_v4().to_string();
    let paths_json = serde_json::to_string(&paths).unwrap_or_default();
    let now = Utc::now().to_rfc3339();

    let _ = sqlx::query(
        "INSERT INTO db_file_guards (id, label, paths, hash, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&label)
    .bind(&paths_json)
    .bind(&hash)
    .bind(&email)
    .bind(&now)
    .execute(&*state.forum_db)
    .await;

    *state.db_file_guards.write().await =
        crate::file_gate::load_db_file_guards(&state.forum_db).await;

    Redirect::to(&format!("{}?flash=File+gate+added&tab=file-gates", dash_path)).into_response()
}

pub async fn delete_file_guard(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<DeleteFileGuardForm>,
    dash_path: String,
) -> impl IntoResponse {
    if let Err(r) = require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        return r;
    }

    let _ = sqlx::query("DELETE FROM db_file_guards WHERE id = ?")
        .bind(&form.id)
        .execute(&*state.forum_db)
        .await;

    *state.db_file_guards.write().await =
        crate::file_gate::load_db_file_guards(&state.forum_db).await;

    Redirect::to(&format!("{}?flash=File+gate+removed&tab=file-gates", dash_path)).into_response()
}

pub async fn set_subscription(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<SetSubscriptionForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (email, _) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };

    let target = form.email.trim().to_lowercase();
    if target.is_empty() || !target.contains('@') {
        let url = format!("{}?flash_error=Invalid+email+address&tab=accounts", dash_path);
        return Redirect::to(&url).into_response();
    }
    let status = match form.status.trim() {
        "active" => "active",
        "expired" => "expired",
        _ => "none",
    };
    let tier = form.tier.trim().to_string();
    let expires_at = form.expires_at.trim();
    let expires_at: Option<String> = if expires_at.is_empty() { None } else { Some(expires_at.to_string()) };
    let now = Utc::now().to_rfc3339();

    let _ = sqlx::query(
        "INSERT INTO account_subscriptions (email, tier, status, expires_at, updated_by, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT(email) DO UPDATE SET tier = excluded.tier, status = excluded.status, \
         expires_at = excluded.expires_at, updated_by = excluded.updated_by, updated_at = excluded.updated_at",
    )
    .bind(&target)
    .bind(&tier)
    .bind(status)
    .bind(&expires_at)
    .bind(&email)
    .bind(&now)
    .execute(&*state.forum_db)
    .await;

    Redirect::to(&format!("{}?flash=Subscription+updated&tab=accounts", dash_path)).into_response()
}

pub async fn grant_writer(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<GrantWriterForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (email, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can manage the Writer tag").into_response();
    }

    let target = form.email.trim().to_lowercase();
    if target.is_empty() || !target.contains('@') {
        let url = format!("{}?flash_error=Invalid+email+address&tab=accounts", dash_path);
        return Redirect::to(&url).into_response();
    }

    // Skip the insert if this email is already a writer (via config.toml or
    // an existing DB rule) so repeated clicks don't pile up duplicate rows.
    let already_writer = {
        let rules = state.access_rules.read().await;
        auth_guard::email_is_writer(&state.config.auth_guards, &rules, &target)
    };
    if !already_writer {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let _ = sqlx::query(
            "INSERT INTO access_rules (id, domain, path, email, email_domain, tag_type, tag_value, created_by, created_at) \
             VALUES (?, '', '/blog/new', ?, NULL, NULL, NULL, ?, ?)",
        )
        .bind(&id)
        .bind(&target)
        .bind(&email)
        .bind(&now)
        .execute(&*state.forum_db)
        .await;
        *state.access_rules.write().await = auth_guard::load_access_rules(&state.forum_db).await;
    }

    Redirect::to(&format!("{}?flash=Writer+access+granted&tab=accounts", dash_path)).into_response()
}

pub async fn revoke_writer(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<RevokeWriterForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (_, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can manage the Writer tag").into_response();
    }

    // Only ever deletes a DB access_rules row for this exact email — a
    // config.toml auth_guard or an email_domain-wide rule can't be touched
    // from here, so a writer status coming from either of those is left as-is.
    let target = form.email.trim().to_lowercase();
    let _ = sqlx::query("DELETE FROM access_rules WHERE path = '/blog/new' AND email = ?")
        .bind(&target)
        .execute(&*state.forum_db)
        .await;
    *state.access_rules.write().await = auth_guard::load_access_rules(&state.forum_db).await;

    Redirect::to(&format!("{}?flash=Writer+access+removed&tab=accounts", dash_path)).into_response()
}

fn private_redirect(dash_path: &str, result: Result<&str, String>) -> axum::response::Response {
    let query = match result {
        Ok(msg) => format!("flash={}", urlencode(msg)),
        Err(e) => format!("flash_error={}", urlencode(&e)),
    };
    Redirect::to(&format!("{}?{}&tab=private", dash_path, query)).into_response()
}

/// Owner check shared by the private media account handlers. Returns the
/// owner's email or the response to send instead.
async fn require_private_owner(
    state: &Arc<AppState>,
    jar: &CookieJar,
    headers: &HeaderMap,
    dash_path: &str,
) -> Result<String, axum::response::Response> {
    let (email, owner) = require_dashboard_access(state, jar, headers, dash_path).await?;
    if !owner {
        return Err((StatusCode::FORBIDDEN, "Only owners can manage private media accounts").into_response());
    }
    Ok(email)
}

pub async fn create_private_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<PrivateAccountForm>,
    dash_path: String,
) -> impl IntoResponse {
    let email = match require_private_owner(&state, &jar, &headers, &dash_path).await {
        Ok(e) => e,
        Err(r) => return r,
    };
    let result = crate::private_media::admin_create_account(
        &state.forum_db, &form.username, &form.password, &email,
    ).await;
    private_redirect(&dash_path, result.map(|_| "Private media account created"))
}

pub async fn reset_private_password(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<PresentPasswordForm>,
    dash_path: String,
) -> impl IntoResponse {
    if let Err(r) = require_private_owner(&state, &jar, &headers, &dash_path).await {
        return r;
    }
    let result = crate::private_media::admin_reset_password(&state.forum_db, &form.id, &form.password).await;
    private_redirect(&dash_path, result.map(|_| "Password reset - existing sessions signed out"))
}

pub async fn suspend_private_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<PresentAccountIdForm>,
    dash_path: String,
) -> impl IntoResponse {
    if let Err(r) = require_private_owner(&state, &jar, &headers, &dash_path).await {
        return r;
    }
    crate::private_media::admin_set_status(&state.forum_db, &form.id, false).await;
    private_redirect(&dash_path, Ok("Account suspended"))
}

pub async fn unsuspend_private_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<PresentAccountIdForm>,
    dash_path: String,
) -> impl IntoResponse {
    if let Err(r) = require_private_owner(&state, &jar, &headers, &dash_path).await {
        return r;
    }
    crate::private_media::admin_set_status(&state.forum_db, &form.id, true).await;
    private_redirect(&dash_path, Ok("Account reactivated"))
}

pub async fn delete_private_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<PresentAccountIdForm>,
    dash_path: String,
) -> impl IntoResponse {
    if let Err(r) = require_private_owner(&state, &jar, &headers, &dash_path).await {
        return r;
    }
    crate::private_media::admin_delete_account(&state.forum_db, &form.id).await;
    private_redirect(&dash_path, Ok("Account deleted"))
}

pub async fn create_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<CreateAccountForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (_, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can create accounts").into_response();
    }

    let organization = form.organization.trim().to_string();
    let target = form.email.trim().to_lowercase();
    if organization.is_empty() {
        let url = format!("{}?flash_error=Organization+is+required&tab=accounts", dash_path);
        return Redirect::to(&url).into_response();
    }
    if target.is_empty() || !target.contains('@') {
        let url = format!("{}?flash_error=Invalid+email+address&tab=accounts", dash_path);
        return Redirect::to(&url).into_response();
    }
    let npi_input = form.npi.as_deref().unwrap_or("").trim().to_string();
    if !npi_input.is_empty() && !crate::npi::is_valid_format(&npi_input) {
        let url = format!("{}?flash_error=NPI+must+be+exactly+10+digits&tab=accounts", dash_path);
        return Redirect::to(&url).into_response();
    }
    let npi_to_store = if npi_input.is_empty() { None } else { Some(npi_input) };

    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM portal_accounts WHERE email = ?")
        .bind(&target)
        .fetch_one(&*state.forum_db)
        .await
        .unwrap_or(0);
    if existing > 0 {
        let url = format!("{}?flash_error=An+account+with+this+email+already+exists&tab=accounts", dash_path);
        return Redirect::to(&url).into_response();
    }

    // No one ever needs this password: the account is created pre-verified,
    // and this action immediately emails a password-setup link, reusing the
    // "forgot password" flow rather than a self-registration flow.
    let id = Uuid::new_v4().to_string();
    let placeholder_password = Uuid::new_v4().to_string();
    let hashed = match bcrypt::hash(&placeholder_password, bcrypt::DEFAULT_COST) {
        Ok(h) => h,
        Err(_) => {
            let url = format!("{}?flash_error=Failed+to+create+account&tab=accounts", dash_path);
            return Redirect::to(&url).into_response();
        }
    };
    let verification_token = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let inserted = sqlx::query(
        "INSERT INTO portal_accounts (id, organization, email, password_hash, is_verified, verification_token, created_at, npi, status) \
         VALUES (?, ?, ?, ?, 1, ?, ?, ?, 'active')",
    )
    .bind(&id)
    .bind(&organization)
    .bind(&target)
    .bind(&hashed)
    .bind(&verification_token)
    .bind(&now)
    .bind(&npi_to_store)
    .execute(&*state.forum_db)
    .await;

    if inserted.is_err() {
        let url = format!("{}?flash_error=Failed+to+create+account&tab=accounts", dash_path);
        return Redirect::to(&url).into_response();
    }

    let reset_token = Uuid::new_v4().to_string();
    let expires = Utc::now() + chrono::Duration::hours(1);
    let _ = sqlx::query(
        "INSERT INTO portal_password_resets (token, account_id, created_at, expires_at, used) \
         VALUES (?, ?, ?, ?, 0)",
    )
    .bind(&reset_token)
    .bind(&id)
    .bind(&now)
    .bind(expires.to_rfc3339())
    .execute(&*state.forum_db)
    .await;

    if let Err(e) = crate::portal::send_password_reset_email(&state.config, &target, &reset_token).await {
        eprintln!("Failed to send account-setup email to {}: {}", target, e);
    }

    Redirect::to(&format!("{}?flash=Account+created+-+a+password+setup+email+was+sent&tab=accounts", dash_path)).into_response()
}

pub async fn remove_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<RemoveAccountForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (_, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can remove accounts").into_response();
    }

    // Mirrors scripts/portal/delete-account.sh: sessions, documents, password
    // reset tokens, the account row, then its uploaded files on disk.
    let target = form.email.trim().to_lowercase();
    let account_id: Option<String> = sqlx::query_scalar("SELECT id FROM portal_accounts WHERE email = ?")
        .bind(&target)
        .fetch_optional(&*state.forum_db)
        .await
        .unwrap_or(None);

    if let Some(id) = account_id {
        let _ = sqlx::query("DELETE FROM portal_sessions WHERE account_id = ?")
            .bind(&id).execute(&*state.forum_db).await;
        let _ = sqlx::query("DELETE FROM portal_documents WHERE account_id = ?")
            .bind(&id).execute(&*state.forum_db).await;
        let _ = sqlx::query("DELETE FROM portal_password_resets WHERE account_id = ?")
            .bind(&id).execute(&*state.forum_db).await;
        let _ = sqlx::query("DELETE FROM portal_accounts WHERE id = ?")
            .bind(&id).execute(&*state.forum_db).await;
        let _ = tokio::fs::remove_dir_all(format!("portal_uploads/{}", id)).await;
    }

    Redirect::to(&format!("{}?flash=Account+removed&tab=accounts", dash_path)).into_response()
}

pub async fn suspend_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<SuspendAccountForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (_, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can suspend accounts").into_response();
    }

    // Blocks future logins immediately, and kills any session already in
    // progress so suspension takes effect right away, not just on next login.
    let target = form.email.trim().to_lowercase();
    let _ = sqlx::query("UPDATE portal_accounts SET status = 'suspended' WHERE email = ?")
        .bind(&target)
        .execute(&*state.forum_db)
        .await;
    let _ = sqlx::query(
        "DELETE FROM portal_sessions WHERE account_id = (SELECT id FROM portal_accounts WHERE email = ?)",
    )
    .bind(&target)
    .execute(&*state.forum_db)
    .await;

    Redirect::to(&format!("{}?flash=Account+suspended&tab=accounts", dash_path)).into_response()
}

pub async fn unsuspend_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<UnsuspendAccountForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (_, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can suspend accounts").into_response();
    }

    let target = form.email.trim().to_lowercase();
    let _ = sqlx::query("UPDATE portal_accounts SET status = 'active' WHERE email = ?")
        .bind(&target)
        .execute(&*state.forum_db)
        .await;

    Redirect::to(&format!("{}?flash=Account+reactivated&tab=accounts", dash_path)).into_response()
}

// ── Interactive presentation accounts ────────────────────────────────────────
// Invite-only: this tab is the only place these accounts are created.

fn present_redirect(dash_path: &str, result: Result<&str, String>) -> axum::response::Response {
    let query = match result {
        Ok(msg) => format!("flash={}", urlencode(msg)),
        Err(e) => format!("flash_error={}", urlencode(&e)),
    };
    Redirect::to(&format!("{}?{}&tab=presentations", dash_path, query)).into_response()
}

pub async fn create_present_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<PresentAccountForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (email, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can manage presentation accounts").into_response();
    }
    let result = crate::presentation::admin_create_account(
        &state.forum_db, &form.username, &form.display_name, &form.password, &email,
    ).await;
    present_redirect(&dash_path, result.map(|_| "Presentation account created"))
}

pub async fn reset_present_password(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<PresentPasswordForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (_, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can manage presentation accounts").into_response();
    }
    let result = crate::presentation::admin_reset_password(&state.forum_db, &form.id, &form.password).await;
    present_redirect(&dash_path, result.map(|_| "Password reset - existing sessions signed out"))
}

pub async fn suspend_present_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<PresentAccountIdForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (_, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can manage presentation accounts").into_response();
    }
    crate::presentation::admin_set_status(&state.forum_db, &form.id, false).await;
    present_redirect(&dash_path, Ok("Account suspended"))
}

pub async fn unsuspend_present_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<PresentAccountIdForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (_, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can manage presentation accounts").into_response();
    }
    crate::presentation::admin_set_status(&state.forum_db, &form.id, true).await;
    present_redirect(&dash_path, Ok("Account reactivated"))
}

pub async fn delete_present_account(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<PresentAccountIdForm>,
    dash_path: String,
) -> impl IntoResponse {
    let (_, owner) = match require_dashboard_access(&state, &jar, &headers, &dash_path).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !owner {
        return (StatusCode::FORBIDDEN, "Only owners can manage presentation accounts").into_response();
    }
    crate::presentation::admin_delete_account(&state.forum_db, &form.id).await;
    present_redirect(&dash_path, Ok("Account and its presentations deleted"))
}
