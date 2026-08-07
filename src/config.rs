use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use solarized::{
    print_fancy,
    ORANGE, RED,
    BOLD,
    PrintMode::NewLine,
};

fn default_renewal_days() -> u32 {
    30
}

fn default_site_name() -> String {
    "Webify".to_string()
}

fn default_http_mode() -> String {
    "serve".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AdminDashboard {
    /// Hostname this dashboard applies to. Empty means all domains.
    #[serde(default)]
    pub domain: String,
    /// URL path prefix for this dashboard (e.g. "/admin/dashboard").
    pub path: String,
    /// Email addresses of owners. Only modifiable via config, not the dashboard UI.
    #[serde(default)]
    pub owners: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct FileGuard {
    /// URL path prefixes this guard protects.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Lowercase hex SHA-256 digest of the accepted key file.
    pub hash: String,
    /// Human-readable label shown in the admin dashboard.
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AuthGuard {
    /// Hostnames this guard applies to. Empty means all domains.
    #[serde(default)]
    pub sites: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub allowed_emails: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PermanentRoom {
    pub name: String,
    pub max_controllers: usize,
    pub max_doers: usize,
    pub password: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub scope: String,
    pub ip: String,
    pub port: u16,
    pub ssl_enabled: bool,
    pub ssl_port: u16,
    pub ssl_cert_path: Option<String>,
    pub ssl_key_path: Option<String>,
    /// Obtain/renew the SSL cert automatically via ACME (Let's Encrypt) using the
    /// HTTP-01 challenge served on the plain-HTTP listener. Writes the issued cert
    /// chain and key to `ssl_cert_path` / `ssl_key_path`. Requires `ssl_enabled`.
    pub acme_enabled: bool,
    /// Domains to request the certificate for (the first is the primary).
    pub acme_domains: Vec<String>,
    /// Contact email registered with the ACME account (optional but recommended).
    pub acme_contact_email: Option<String>,
    /// Use the Let's Encrypt production directory when true, staging when false.
    pub acme_production: bool,
    /// Where the persisted ACME account credentials are stored (JSON). Reused
    /// across restarts so we don't register a new account every time.
    pub acme_account_path: Option<String>,
    /// Renew when fewer than this many days remain before the cert expires.
    pub acme_renewal_days: u32,
    /// Behaviour of the plain-HTTP listener: "serve" (full app over HTTP),
    /// "redirect" (308 to HTTPS), or "https_only" (only answer ACME challenges).
    pub http_mode: String,
    pub upload_size_limit: Option<Value>,
    pub upload_storage_limit: Option<u64>,
    pub browser: bool,
    pub sites: HashMap<String, Vec<(String, Vec<String>)>>,
    pub whitelists: HashMap<String, Vec<String>>,
    pub slideshow_autoplay: bool,
    pub slideshow_timer: u64,
    pub domain: String,
    /// Human-readable product/site name used in outgoing email copy (e.g.
    /// "Verify your {site_name} account"). Defaults to "Webify" for a bare
    /// install — set this in config.toml to your own brand.
    pub site_name: String,
    pub smtp_server: Option<String>,
    pub smtp_port: Option<u16>,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
    pub email_from: Option<String>,
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub google_redirect_url: Option<String>,
    pub permanent_rooms: Option<Vec<PermanentRoom>>,
    pub public_ip: Option<String>,
    pub auth_guards: Vec<AuthGuard>,
    pub guard_redirect_url: Option<String>,
    pub guard_login_page: Option<String>,
    /// Domain attribute for the guard session cookie (e.g. ".example.com").
    /// Set this to a registrable parent domain to share the sign-in session across
    /// sibling subdomains (so one OAuth callback covers every host in the same
    /// service). Leave unset for a host-only cookie.
    pub guard_cookie_domain: Option<String>,
    pub admin_dashboards: Vec<AdminDashboard>,
    pub file_guards: Vec<FileGuard>,
    /// Per-account storage quota (bytes) for the client portal's document
    /// uploads. Defaults to 500MB (see `portal::DEFAULT_PORTAL_ACCOUNT_QUOTA`)
    /// when unset.
    pub portal_storage_limit_per_account: Option<u64>,
    /// Stripe publishable key (safe to expose client-side).
    pub stripe_publishable_key: Option<String>,
    /// Stripe secret key. Never commit a real value to a tracked config file —
    /// set this only in the gitignored `config.toml` on each machine/server.
    pub stripe_secret_key: Option<String>,
    /// Signing secret for the `/stripe/webhook` endpoint (from the Stripe
    /// webhook_endpoint object), used to verify the `Stripe-Signature` header.
    /// Same secrecy rules as `stripe_secret_key`.
    pub stripe_webhook_secret: Option<String>,
    /// Price ID for the $99/mo "Standard" subscription + pay-as-you-go tier.
    pub stripe_price_standard: Option<String>,
    /// Price ID for the $499/mo "Pro" subscription tier.
    pub stripe_price_pro: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    scope: String,
    ip: String,
    port: u16,
    ssl_enabled: bool,
    ssl_port: u16,
    ssl_cert_path: Option<String>,
    ssl_key_path: Option<String>,
    #[serde(default)]
    acme_enabled: bool,
    #[serde(default)]
    acme_domains: Vec<String>,
    #[serde(default)]
    acme_contact_email: Option<String>,
    #[serde(default)]
    acme_production: bool,
    #[serde(default)]
    acme_account_path: Option<String>,
    #[serde(default = "default_renewal_days")]
    acme_renewal_days: u32,
    #[serde(default = "default_http_mode")]
    http_mode: String,
    upload_size_limit: Option<Value>,
    upload_storage_limit: Option<u64>,
    browser: bool,
    slideshow_autoplay: bool,
    slideshow_timer: u64,
    domain: String,
    #[serde(default = "default_site_name")]
    site_name: String,
    smtp_server: Option<String>,
    smtp_port: Option<u16>,
    smtp_username: Option<String>,
    smtp_password: Option<String>,
    email_from: Option<String>,
    google_client_id: Option<String>,
    google_client_secret: Option<String>,
    google_redirect_url: Option<String>,
    pub permanent_rooms: Option<Vec<PermanentRoom>>,
    pub public_ip: Option<String>,
    pub guard_redirect_url: Option<String>,
    #[serde(default)]
    pub guard_login_page: Option<String>,
    #[serde(default)]
    pub guard_cookie_domain: Option<String>,
    #[serde(default)]
    pub auth_guard: Vec<AuthGuard>,
    #[serde(default, rename = "admin_dashboard")]
    pub admin_dashboard: Vec<AdminDashboard>,
    #[serde(default, rename = "file_guard")]
    pub file_guard: Vec<FileGuard>,
    #[serde(default)]
    pub portal_storage_limit_per_account: Option<u64>,
    #[serde(default)]
    pub stripe_publishable_key: Option<String>,
    #[serde(default)]
    pub stripe_secret_key: Option<String>,
    #[serde(default)]
    pub stripe_webhook_secret: Option<String>,
    #[serde(default)]
    pub stripe_price_standard: Option<String>,
    #[serde(default)]
    pub stripe_price_pro: Option<String>,
    #[serde(default)]
    routes: HashMap<String, RouteValue>,
    #[serde(default)]
    whitelist: HashMap<String, WhitelistValue>,
}

/// Overlay for secrets that must never live in a tracked config file (e.g.
/// `prod-config.toml`, which a GitHub Actions deploy copies verbatim onto the
/// droplet). Read from a gitignored `config.secrets.toml` sitting next to
/// `config.toml` and merged in after the main config parses, so the tracked
/// file only ever holds non-sensitive settings (routes, price IDs, etc.).
#[derive(Debug, Deserialize, Default)]
struct RawSecrets {
    #[serde(default)]
    stripe_secret_key: Option<String>,
    #[serde(default)]
    stripe_webhook_secret: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RouteValue {
    Settings(Vec<String>),
    DomainMap(HashMap<String, Vec<String>>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WhitelistValue {
    Ips(Vec<String>),
    DomainMap(HashMap<String, Vec<String>>),
}

pub fn read_config() -> Option<Config> {
    let contents = fs::read_to_string("config.toml").ok()?;
    let mut raw: RawConfig = toml::from_str(&contents).map_err(|e| {
        print_fancy(&[
            ("Error parsing config file: ", ORANGE, vec![]),
            (&format!("{}", e), RED, vec![BOLD])
        ], NewLine);
    }).ok()?;

    // Merge in the gitignored secrets overlay, if present. Never required —
    // an install with no secrets file just leaves these fields None.
    if let Ok(secrets_contents) = fs::read_to_string("config.secrets.toml") {
        match toml::from_str::<RawSecrets>(&secrets_contents) {
            Ok(secrets) => {
                if secrets.stripe_secret_key.is_some() {
                    raw.stripe_secret_key = secrets.stripe_secret_key;
                }
                if secrets.stripe_webhook_secret.is_some() {
                    raw.stripe_webhook_secret = secrets.stripe_webhook_secret;
                }
            }
            Err(e) => {
                print_fancy(&[
                    ("Error parsing config.secrets.toml: ", ORANGE, vec![]),
                    (&format!("{}", e), RED, vec![BOLD])
                ], NewLine);
            }
        }
    }
    let mut sites = HashMap::new();
    for (key, value) in raw.routes {
        match value {
            RouteValue::Settings(s) => {
                sites.entry("default".to_string()).or_insert_with(Vec::new).push((key, s));
            }
            RouteValue::DomainMap(map) => {
                let domain_routes = sites.entry(key).or_insert_with(Vec::new);
                for (path, s) in map {
                    domain_routes.push((path, s));
                }
            }
        }
    }
    let mut whitelists = HashMap::new();
    for (key, value) in raw.whitelist {
        match value {
            WhitelistValue::Ips(ips) => {
                whitelists.entry(key).or_insert_with(Vec::new).extend(ips);
            }
            WhitelistValue::DomainMap(map) => {
                let domain_ips = whitelists.entry(key).or_insert_with(Vec::new);
                for (_, ips) in map {
                    domain_ips.extend(ips);
                }
            }
        }
    }
    Some(Config {
        scope: raw.scope,
        ip: raw.ip,
        port: raw.port,
        ssl_enabled: raw.ssl_enabled,
        ssl_port: raw.ssl_port,
        ssl_cert_path: raw.ssl_cert_path,
        ssl_key_path: raw.ssl_key_path,
        acme_enabled: raw.acme_enabled,
        acme_domains: raw.acme_domains,
        acme_contact_email: raw.acme_contact_email,
        acme_production: raw.acme_production,
        acme_account_path: raw.acme_account_path,
        acme_renewal_days: raw.acme_renewal_days,
        http_mode: raw.http_mode.trim().to_lowercase(),
        upload_size_limit: raw.upload_size_limit,
        upload_storage_limit: raw.upload_storage_limit,
        browser: raw.browser,
        sites,
        whitelists,
        slideshow_autoplay: raw.slideshow_autoplay,
        slideshow_timer: raw.slideshow_timer,
        domain: raw.domain.trim().to_string(),
        site_name: raw.site_name.trim().to_string(),
        smtp_server: raw.smtp_server,
        smtp_port: raw.smtp_port,
        smtp_username: raw.smtp_username,
        smtp_password: raw.smtp_password,
        email_from: raw.email_from,
        google_client_id: raw.google_client_id,
        google_client_secret: raw.google_client_secret,
        google_redirect_url: raw.google_redirect_url,
        permanent_rooms: raw.permanent_rooms,
        public_ip: raw.public_ip,
        auth_guards: raw.auth_guard,
        guard_redirect_url: raw.guard_redirect_url,
        guard_login_page: raw.guard_login_page,
        guard_cookie_domain: raw.guard_cookie_domain,
        admin_dashboards: raw.admin_dashboard,
        file_guards: raw.file_guard,
        portal_storage_limit_per_account: raw.portal_storage_limit_per_account,
        stripe_publishable_key: raw.stripe_publishable_key,
        stripe_secret_key: raw.stripe_secret_key,
        stripe_webhook_secret: raw.stripe_webhook_secret,
        stripe_price_standard: raw.stripe_price_standard,
        stripe_price_pro: raw.stripe_price_pro,
    })
}
