use axum::{
    extract::{State, Query, Multipart},
    response::{Html, IntoResponse, Redirect},
    http::StatusCode,
};
use sha2::{Sha256, Digest};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use crate::AppState;
use crate::config::FileGuard;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use chrono::{Utc, Duration};

pub const FILE_GATE_COOKIE: &str = "file_gate_token";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbFileGuard {
    pub id: String,
    pub label: String,
    pub paths: Vec<String>,
    pub hash: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct FileGateQuery {
    pub next: Option<String>,
    pub error: Option<String>,
}

fn safe_next(raw: Option<String>) -> String {
    match raw {
        Some(s) if s.starts_with('/') && !s.starts_with("//") => s,
        _ => "/".to_string(),
    }
}

fn path_matches(guard_path: &str, request_path: &str) -> bool {
    let prefix = guard_path.trim_end_matches('/');
    request_path == prefix || request_path.starts_with(&format!("{}/", prefix))
}

pub fn find_required_hash_config(guards: &[FileGuard], path: &str) -> Option<String> {
    guards.iter().find(|g| {
        g.paths.iter().any(|p| path_matches(p, path))
    }).map(|g| g.hash.clone())
}

pub fn find_required_hash_db(guards: &[DbFileGuard], path: &str) -> Option<String> {
    guards.iter().find(|g| {
        g.paths.iter().any(|p| path_matches(p, path))
    }).map(|g| g.hash.clone())
}

pub async fn load_db_file_guards(db: &crate::forum::ForumDb) -> Vec<DbFileGuard> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: String,
        label: String,
        paths: String,
        hash: String,
        created_by: String,
        created_at: String,
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, label, paths, hash, created_by, created_at FROM db_file_guards ORDER BY created_at DESC"
    )
    .fetch_all(&**db)
    .await
    .unwrap_or_default();

    rows.into_iter().map(|r| DbFileGuard {
        id: r.id,
        label: r.label,
        paths: serde_json::from_str(&r.paths).unwrap_or_default(),
        hash: r.hash,
        created_by: r.created_by,
        created_at: r.created_at,
    }).collect()
}

pub async fn validate_session(db: &crate::forum::ForumDb, token: &str, required_hash: &str) -> bool {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT gate_hash, expires_at FROM file_gate_sessions WHERE token = ?"
    )
    .bind(token)
    .fetch_optional(&**db)
    .await
    .unwrap_or(None);

    match row {
        Some((gate_hash, expires_str)) if gate_hash == required_hash => {
            if let Ok(expires) = chrono::DateTime::parse_from_rfc3339(&expires_str) {
                if expires >= Utc::now() {
                    return true;
                }
            }
            let _ = sqlx::query("DELETE FROM file_gate_sessions WHERE token = ?")
                .bind(token)
                .execute(&**db)
                .await;
            false
        }
        _ => false,
    }
}

async fn create_session(db: &crate::forum::ForumDb, gate_hash: &str) -> String {
    let token = Uuid::new_v4().to_string();
    let now = Utc::now();
    let expires = now + Duration::days(30);
    let _ = sqlx::query(
        "INSERT INTO file_gate_sessions (token, gate_hash, created_at, expires_at) VALUES (?, ?, ?, ?)"
    )
    .bind(&token)
    .bind(gate_hash)
    .bind(now.to_rfc3339())
    .bind(expires.to_rfc3339())
    .execute(&**db)
    .await;
    token
}

pub async fn file_gate_page(
    State(state): State<Arc<AppState>>,
    Query(q): Query<FileGateQuery>,
) -> impl IntoResponse {
    let next = safe_next(q.next);
    let error = q.error.is_some();

    let db_guards = state.db_file_guards.read().await;
    let required = find_required_hash_config(&state.config.file_guards, &next)
        .or_else(|| find_required_hash_db(&db_guards, &next));
    drop(db_guards);

    if required.is_none() {
        return Redirect::to(&next).into_response();
    }

    let mut ctx = tera::Context::new();
    ctx.insert("next", &next);
    ctx.insert("error", &error);

    match state.tera.render("file-gate.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Gate template missing — add static/file-gate.html").into_response(),
    }
}

pub async fn file_gate_submit(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(q): Query<FileGateQuery>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let next = safe_next(q.next);
    let err_url = format!("/auth/file-gate?next={}&error=1", urlencoding::encode(&next));

    let mut file_bytes: Option<Vec<u8>> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name().unwrap_or("") == "key_file" {
            if let Ok(bytes) = field.bytes().await {
                if !bytes.is_empty() {
                    file_bytes = Some(bytes.to_vec());
                }
                break;
            }
        }
    }

    let bytes = match file_bytes {
        Some(b) => b,
        None => return Redirect::to(&err_url).into_response(),
    };

    let uploaded_hash = format!("{:x}", Sha256::digest(&bytes));

    let db_guards = state.db_file_guards.read().await;
    let required_hash = find_required_hash_config(&state.config.file_guards, &next)
        .or_else(|| find_required_hash_db(&db_guards, &next));
    drop(db_guards);

    match required_hash {
        Some(h) if h == uploaded_hash => {
            let token = create_session(&state.forum_db, &h).await;
            let cookie = Cookie::build((FILE_GATE_COOKIE, token))
                .path("/")
                .http_only(true)
                .build();
            (jar.add(cookie), Redirect::to(&next)).into_response()
        }
        _ => Redirect::to(&err_url).into_response(),
    }
}
