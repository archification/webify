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

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AccessRuleRow {
    pub id: String,
    pub domain: String,
    pub path: String,
    pub email: Option<String>,
    pub email_domain: Option<String>,
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
        "SELECT id, domain, path, email, email_domain, created_by, created_at \
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

    let mut ctx = tera::Context::new();
    ctx.insert("email", &email);
    ctx.insert("is_owner", &owner);
    ctx.insert("rules", &rules);
    ctx.insert("editors", &editors);
    ctx.insert("dashboard_path", &dash_path);
    ctx.insert("flash", &flash);
    ctx.insert("flash_error", &flash_error);

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

    if email_val.is_none() && domain_val.is_none() {
        let url = format!("{}?flash_error=Specify+at+least+one+email+or+email+domain", dash_path);
        return Redirect::to(&url).into_response();
    }

    let id = Uuid::new_v4().to_string();
    let domain = form.domain.trim().to_lowercase();
    let now = Utc::now().to_rfc3339();

    let _ = sqlx::query(
        "INSERT INTO access_rules (id, domain, path, email, email_domain, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&domain)
    .bind(&path)
    .bind(&email_val)
    .bind(&domain_val)
    .bind(&email)
    .bind(&now)
    .execute(&*state.forum_db)
    .await;

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
