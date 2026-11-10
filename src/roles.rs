// Role-based permissions for signed-in (Google OAuth guard session) users.
// Roles are granted per email from the admin dashboard's Users tab. Owners
// come from `[[admin_dashboard]] owners` in config.toml, implicitly hold every
// role, and are the only ones who can grant or revoke roles.

use crate::config::Config;
use crate::forum::ForumDb;
use chrono::Utc;
use serde::Serialize;
use std::collections::HashMap;

/// Every role the dashboard can assign: (id stored in the DB, label, description).
pub const ROLES: &[(&str, &str, &str)] = &[
    ("writer", "Writer", "Can write blog posts and edit their own"),
];

pub const WRITER: &str = "writer";

#[derive(Serialize)]
pub struct RoleInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

pub fn role_list() -> Vec<RoleInfo> {
    ROLES.iter().map(|&(id, label, description)| RoleInfo { id, label, description }).collect()
}

pub fn is_known_role(role: &str) -> bool {
    ROLES.iter().any(|(id, _, _)| *id == role)
}

/// Whether `email` is an owner of any admin dashboard in config.toml.
pub fn is_owner(config: &Config, email: &str) -> bool {
    config.admin_dashboards.iter()
        .flat_map(|d| d.owners.iter())
        .any(|o| o.eq_ignore_ascii_case(email))
}

pub async fn init_tables(db: &ForumDb) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS user_roles (
            email TEXT NOT NULL,
            role TEXT NOT NULL,
            granted_by TEXT NOT NULL,
            granted_at TEXT NOT NULL,
            PRIMARY KEY (email, role)
        )"
    ).execute(&**db).await.unwrap();

    // Migration: writers used to be per-email access_rules rows on /blog/new.
    // Carry them over as the writer role and drop the rows, since a leftover
    // rule on /blog/new would gate the editor to just those emails.
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO user_roles (email, role, granted_by, granted_at) \
         SELECT lower(email), 'writer', created_by, created_at FROM access_rules \
         WHERE path = '/blog/new' AND email IS NOT NULL",
    ).execute(&**db).await;
    let _ = sqlx::query("DELETE FROM access_rules WHERE path = '/blog/new' AND email IS NOT NULL")
        .execute(&**db).await;
}

/// Whether `email` holds `role`, either granted in the DB or as an owner.
pub async fn has_role(config: &Config, db: &ForumDb, email: &str, role: &str) -> bool {
    if is_owner(config, email) {
        return true;
    }
    sqlx::query_scalar::<_, bool>("SELECT COUNT(*) > 0 FROM user_roles WHERE email = ? AND role = ?")
        .bind(email.to_ascii_lowercase())
        .bind(role)
        .fetch_one(&**db)
        .await
        .unwrap_or(false)
}

/// All granted roles, keyed by lowercase email.
pub async fn all_roles(db: &ForumDb) -> HashMap<String, Vec<String>> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT email, role FROM user_roles ORDER BY role")
        .fetch_all(&**db)
        .await
        .unwrap_or_default();
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for (email, role) in rows {
        out.entry(email).or_default().push(role);
    }
    out
}

pub async fn grant(db: &ForumDb, email: &str, role: &str, granted_by: &str) {
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO user_roles (email, role, granted_by, granted_at) VALUES (?, ?, ?, ?)",
    )
    .bind(email.to_ascii_lowercase())
    .bind(role)
    .bind(granted_by)
    .bind(Utc::now().to_rfc3339())
    .execute(&**db)
    .await;
}

pub async fn revoke(db: &ForumDb, email: &str, role: &str) {
    let _ = sqlx::query("DELETE FROM user_roles WHERE email = ? AND role = ?")
        .bind(email.to_ascii_lowercase())
        .bind(role)
        .execute(&**db)
        .await;
}
