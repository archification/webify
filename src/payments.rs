// Stripe Checkout + Customer Portal + webhook integration for the client
// portal's paid tiers. Entitlement state lives in the existing
// `account_subscriptions` table (forum.rs) — the same table the admin
// dashboard's manual "Set Subscription" form already writes to (keyed by
// email, with a `tier`/`status`/`expires_at` shape `auth_guard::email_matches_tier`
// already reads). Stripe just becomes another writer, driven by webhooks
// instead of a human.
//
// No Stripe SDK crate is vendored — this talks to the Stripe HTTP API
// directly via `reqwest` (same pattern as `npi.rs`) and verifies webhook
// signatures by hand with `hmac`+`sha2`. See
// https://stripe.com/docs/webhooks/signatures.
//
// Stripe, not the client-side success redirect, is treated as the source of
// truth for subscription state: `/portal/subscribe/return` just shows a
// "thanks, hang tight" page rather than granting anything itself.

use axum::{
    body::Bytes,
    extract::{Path as AxPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use crate::portal::get_current_account;
use crate::AppState;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::Value;
use sha2::Sha256;
use std::sync::Arc;

type HmacSha256 = Hmac<Sha256>;

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

struct SubscriptionInfo {
    tier: String,
    status: String,
    expires_at: Option<String>,
    stripe_customer_id: Option<String>,
}

async fn load_subscription(state: &AppState, email: &str) -> SubscriptionInfo {
    let row: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT tier, status, expires_at, stripe_customer_id FROM account_subscriptions WHERE email = ?",
    )
    .bind(email)
    .fetch_optional(&*state.forum_db)
    .await
    .unwrap_or(None);

    match row {
        Some((tier, status, expires_at, stripe_customer_id)) => SubscriptionInfo {
            tier,
            status,
            expires_at,
            stripe_customer_id,
        },
        None => SubscriptionInfo {
            tier: String::new(),
            status: "none".to_string(),
            expires_at: None,
            stripe_customer_id: None,
        },
    }
}

fn tier_price_id(state: &AppState, tier: &str) -> Option<String> {
    match tier {
        "standard" => state.config.stripe_price_standard.clone(),
        "pro" => state.config.stripe_price_pro.clone(),
        _ => None,
    }
}

fn tier_for_price_id(state: &AppState, price_id: &str) -> Option<&'static str> {
    if state.config.stripe_price_standard.as_deref() == Some(price_id) {
        Some("standard")
    } else if state.config.stripe_price_pro.as_deref() == Some(price_id) {
        Some("pro")
    } else {
        None
    }
}

fn site_base_url(state: &AppState) -> String {
    let scheme = if state.config.ssl_enabled { "https" } else { "http" };
    format!("{}://{}", scheme, state.config.domain)
}

// ─── Handlers ────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct BillingQuery {
    pub canceled: Option<String>,
}

pub async fn billing_page(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(q): Query<BillingQuery>,
) -> impl IntoResponse {
    let account = match get_current_account(&state, &jar).await {
        Some(a) => a,
        None => return Redirect::to("/portal/login").into_response(),
    };

    let sub = load_subscription(&state, &account.email).await;
    let mut ctx = tera::Context::new();
    ctx.insert("organization", &account.organization);
    ctx.insert("email", &account.email);
    ctx.insert("tier", &sub.tier);
    ctx.insert("status", &sub.status);
    ctx.insert("expires_at", &sub.expires_at);
    ctx.insert("has_customer", &sub.stripe_customer_id.is_some());
    ctx.insert("billing_configured", &state.config.stripe_secret_key.is_some());
    ctx.insert("canceled", &q.canceled.as_deref().map(|v| v == "1").unwrap_or(false));

    render(&state, "portal-billing.html", &ctx).into_response()
}

pub async fn subscribe(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    AxPath(tier): AxPath<String>,
) -> impl IntoResponse {
    let account = match get_current_account(&state, &jar).await {
        Some(a) => a,
        None => return Redirect::to("/portal/login").into_response(),
    };
    if !account.is_verified {
        return (
            StatusCode::FORBIDDEN,
            "Please verify your email before subscribing.",
        )
            .into_response();
    }

    let Some(secret_key) = state.config.stripe_secret_key.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Billing isn't configured in this environment yet.",
        )
            .into_response();
    };
    let Some(price_id) = tier_price_id(&state, &tier) else {
        return (StatusCode::NOT_FOUND, "Unknown plan.").into_response();
    };

    let base = site_base_url(&state);
    let success_url = format!(
        "{}/portal/subscribe/return?session_id={{CHECKOUT_SESSION_ID}}",
        base
    );
    let cancel_url = format!("{}/portal/billing?canceled=1", base);

    let mut params: Vec<(String, String)> = vec![
        ("mode".into(), "subscription".into()),
        ("line_items[0][price]".into(), price_id),
        ("line_items[0][quantity]".into(), "1".into()),
        ("client_reference_id".into(), account.id.clone()),
        ("success_url".into(), success_url),
        ("cancel_url".into(), cancel_url),
        ("metadata[account_email]".into(), account.email.clone()),
        ("metadata[tier]".into(), tier.clone()),
        (
            "subscription_data[metadata][account_email]".into(),
            account.email.clone(),
        ),
        ("subscription_data[metadata][tier]".into(), tier.clone()),
    ];

    // Reuse the existing Stripe customer on a resubscribe instead of letting
    // Checkout create a duplicate customer record for the same person.
    let existing = load_subscription(&state, &account.email).await;
    match existing.stripe_customer_id {
        Some(customer_id) => params.push(("customer".into(), customer_id)),
        None => params.push(("customer_email".into(), account.email.clone())),
    }

    let client = reqwest::Client::new();
    let resp = match client
        .post("https://api.stripe.com/v1/checkout/sessions")
        .bearer_auth(&secret_key)
        .form(&params)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Stripe checkout session request failed: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                "Could not reach Stripe. Try again shortly.",
            )
                .into_response();
        }
    };

    let status = resp.status();
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => {
            return (StatusCode::BAD_GATEWAY, "Stripe returned an unreadable response.")
                .into_response()
        }
    };

    if !status.is_success() {
        let msg = body
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Stripe rejected the checkout request.");
        eprintln!("Stripe checkout session error: {}", msg);
        return (
            StatusCode::BAD_GATEWAY,
            format!("Could not start checkout: {}", msg),
        )
            .into_response();
    }

    match body.get("url").and_then(|u| u.as_str()) {
        Some(url) => Redirect::to(url).into_response(),
        None => (StatusCode::BAD_GATEWAY, "Stripe did not return a checkout URL.").into_response(),
    }
}

#[derive(Deserialize)]
pub struct ReturnQuery {
    #[allow(dead_code)]
    pub session_id: Option<String>,
}

/// Stripe, not this redirect, is the source of truth for subscription state
/// (see module docs) — this just shows a "hang tight" page while the webhook
/// catches up, rather than granting anything itself.
pub async fn subscribe_return(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(_q): Query<ReturnQuery>,
) -> impl IntoResponse {
    if get_current_account(&state, &jar).await.is_none() {
        return Redirect::to("/portal/login").into_response();
    }
    let ctx = tera::Context::new();
    render(&state, "portal-subscribe-return.html", &ctx).into_response()
}

pub async fn billing_portal(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    let account = match get_current_account(&state, &jar).await {
        Some(a) => a,
        None => return Redirect::to("/portal/login").into_response(),
    };
    let Some(secret_key) = state.config.stripe_secret_key.clone() else {
        return Redirect::to("/portal/billing").into_response();
    };

    let sub = load_subscription(&state, &account.email).await;
    let Some(customer_id) = sub.stripe_customer_id else {
        return Redirect::to("/portal/billing").into_response();
    };

    let return_url = format!("{}/portal/billing", site_base_url(&state));
    let params = [
        ("customer", customer_id.as_str()),
        ("return_url", return_url.as_str()),
    ];

    let client = reqwest::Client::new();
    let resp = match client
        .post("https://api.stripe.com/v1/billing_portal/sessions")
        .bearer_auth(&secret_key)
        .form(&params)
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return Redirect::to("/portal/billing").into_response(),
    };

    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return Redirect::to("/portal/billing").into_response(),
    };

    match body.get("url").and_then(|u| u.as_str()) {
        Some(url) => Redirect::to(url).into_response(),
        None => Redirect::to("/portal/billing").into_response(),
    }
}

// ─── Webhook ─────────────────────────────────────────────────────────────────

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| s.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()))
        .collect()
}

/// Verifies Stripe's `Stripe-Signature` header against the raw request body.
/// See https://stripe.com/docs/webhooks/signatures — timestamps older than 5
/// minutes are rejected to limit replay of a captured payload.
fn verify_stripe_signature(payload: &[u8], sig_header: &str, secret: &str) -> bool {
    let mut timestamp: Option<i64> = None;
    let mut v1_sigs: Vec<&str> = Vec::new();
    for part in sig_header.split(',') {
        let mut kv = part.splitn(2, '=');
        match (kv.next(), kv.next()) {
            (Some("t"), Some(v)) => timestamp = v.parse().ok(),
            (Some("v1"), Some(v)) => v1_sigs.push(v),
            _ => {}
        }
    }
    let Some(t) = timestamp else { return false };
    if (Utc::now().timestamp() - t).abs() > 300 {
        return false;
    }
    if v1_sigs.is_empty() {
        return false;
    }

    let mut signed_payload = Vec::with_capacity(payload.len() + 32);
    signed_payload.extend_from_slice(t.to_string().as_bytes());
    signed_payload.push(b'.');
    signed_payload.extend_from_slice(payload);

    let Ok(mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };

    v1_sigs.iter().any(|sig| {
        let Some(sig_bytes) = hex_decode(sig) else {
            return false;
        };
        let mut mac = mac.clone();
        mac.update(&signed_payload);
        mac.verify_slice(&sig_bytes).is_ok()
    })
}

fn unix_to_date(ts: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(ts, 0).map(|d| d.date_naive().format("%Y-%m-%d").to_string())
}

fn stripe_status_to_local(status: &str) -> &'static str {
    match status {
        "active" | "trialing" => "active",
        _ => "canceled",
    }
}

/// Idempotent by construction: every branch just upserts the current known
/// state for an email, so replaying the same event (Stripe retries on
/// non-2xx) lands on the same row rather than double-applying anything.
async fn upsert_subscription(
    state: &AppState,
    email: &str,
    tier: &str,
    status: &str,
    expires_at: Option<&str>,
    customer_id: Option<&str>,
    subscription_id: Option<&str>,
) {
    let now = Utc::now().to_rfc3339();
    let _ = sqlx::query(
        "INSERT INTO account_subscriptions (email, tier, status, expires_at, updated_by, updated_at, stripe_customer_id, stripe_subscription_id)
         VALUES (?, ?, ?, ?, 'stripe', ?, ?, ?)
         ON CONFLICT(email) DO UPDATE SET
            tier = excluded.tier,
            status = excluded.status,
            expires_at = excluded.expires_at,
            updated_by = excluded.updated_by,
            updated_at = excluded.updated_at,
            stripe_customer_id = COALESCE(excluded.stripe_customer_id, account_subscriptions.stripe_customer_id),
            stripe_subscription_id = COALESCE(excluded.stripe_subscription_id, account_subscriptions.stripe_subscription_id)"
    )
    .bind(email)
    .bind(tier)
    .bind(status)
    .bind(expires_at)
    .bind(&now)
    .bind(customer_id)
    .bind(subscription_id)
    .execute(&*state.forum_db)
    .await;
}

pub async fn webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(secret) = state.config.stripe_webhook_secret.clone() else {
        eprintln!("Stripe webhook received but stripe_webhook_secret is not configured — rejecting.");
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let sig_header = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !verify_stripe_signature(&body, sig_header, &secret) {
        return StatusCode::BAD_REQUEST;
    }

    let event: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return StatusCode::BAD_REQUEST,
    };

    let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let obj = event.pointer("/data/object").cloned().unwrap_or(Value::Null);

    match event_type {
        "checkout.session.completed" => {
            let email = obj
                .get("metadata")
                .and_then(|m| m.get("account_email"))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    obj.get("customer_details")
                        .and_then(|c| c.get("email"))
                        .and_then(|v| v.as_str())
                });
            let tier = obj
                .get("metadata")
                .and_then(|m| m.get("tier"))
                .and_then(|v| v.as_str());
            let customer_id = obj.get("customer").and_then(|v| v.as_str());
            let subscription_id = obj.get("subscription").and_then(|v| v.as_str());
            if let (Some(email), Some(tier)) = (email, tier) {
                upsert_subscription(&state, email, tier, "active", None, customer_id, subscription_id).await;
            }
        }
        "customer.subscription.created" | "customer.subscription.updated" => {
            let email = obj
                .get("metadata")
                .and_then(|m| m.get("account_email"))
                .and_then(|v| v.as_str());
            let meta_tier = obj
                .get("metadata")
                .and_then(|m| m.get("tier"))
                .and_then(|v| v.as_str());
            let price_id = obj.pointer("/items/data/0/price/id").and_then(|v| v.as_str());
            let tier = meta_tier.or_else(|| price_id.and_then(|p| tier_for_price_id(&state, p)));
            let status_raw = obj.get("status").and_then(|v| v.as_str()).unwrap_or("");
            let expires_at = obj
                .get("current_period_end")
                .and_then(|v| v.as_i64())
                .and_then(unix_to_date);
            let customer_id = obj.get("customer").and_then(|v| v.as_str());
            let subscription_id = obj.get("id").and_then(|v| v.as_str());
            if let (Some(email), Some(tier)) = (email, tier) {
                upsert_subscription(
                    &state,
                    email,
                    tier,
                    stripe_status_to_local(status_raw),
                    expires_at.as_deref(),
                    customer_id,
                    subscription_id,
                )
                .await;
            }
        }
        "customer.subscription.deleted" => {
            let email = obj
                .get("metadata")
                .and_then(|m| m.get("account_email"))
                .and_then(|v| v.as_str());
            let meta_tier = obj
                .get("metadata")
                .and_then(|m| m.get("tier"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let customer_id = obj.get("customer").and_then(|v| v.as_str());
            let subscription_id = obj.get("id").and_then(|v| v.as_str());
            if let Some(email) = email {
                upsert_subscription(&state, email, meta_tier, "canceled", None, customer_id, subscription_id).await;
            }
        }
        _ => {}
    }

    StatusCode::OK
}
