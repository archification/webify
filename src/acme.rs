use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum_server::tls_rustls::RustlsConfig;
use chrono::Utc;
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, OrderStatus, RetryPolicy,
};
use tokio::sync::RwLock;
use solarized::{
    print_fancy,
    BLUE, CYAN, GREEN, ORANGE, RED,
    BOLD,
    PrintMode::NewLine,
};

use crate::config::Config;

/// Maps an ACME HTTP-01 challenge token to the key-authorization string that must
/// be served at `/.well-known/acme-challenge/{token}`. Shared between the ACME
/// task (which populates it) and the HTTP route (which reads it).
pub type ChallengeStore = Arc<RwLock<HashMap<String, String>>>;

const ACCOUNT_PATH_DEFAULT: &str = "pems/acme_account.json";

pub fn new_store() -> ChallengeStore {
    Arc::new(RwLock::new(HashMap::new()))
}

fn log_info(msg: &str) {
    print_fancy(&[("[acme] ", BLUE, vec![]), (msg, CYAN, vec![])], NewLine);
}

fn log_ok(msg: &str) {
    print_fancy(&[("[acme] ", BLUE, vec![]), (msg, GREEN, vec![BOLD])], NewLine);
}

fn log_warn(msg: &str) {
    print_fancy(&[("[acme] ", BLUE, vec![]), (msg, ORANGE, vec![])], NewLine);
}

fn log_err(msg: &str) {
    print_fancy(&[("[acme] ", BLUE, vec![]), (msg, RED, vec![BOLD])], NewLine);
}

/// Read the `notAfter` of the first certificate in a PEM file as a unix timestamp.
fn parse_not_after(pem_bytes: &[u8]) -> Option<i64> {
    for pem in x509_parser::pem::Pem::iter_from_buffer(pem_bytes) {
        let pem = pem.ok()?;
        if pem.label == "CERTIFICATE" {
            let cert = pem.parse_x509().ok()?;
            return Some(cert.validity().not_after.timestamp());
        }
    }
    None
}

/// True when the cert file is missing/unreadable, unparseable, or expires within
/// `renewal_days` of now.
pub async fn cert_needs_renewal(cert_path: &str, renewal_days: u32) -> bool {
    let pem = match tokio::fs::read(cert_path).await {
        Ok(p) => p,
        Err(_) => return true,
    };
    match parse_not_after(&pem) {
        Some(not_after) => {
            let threshold = Utc::now().timestamp() + (renewal_days as i64) * 86_400;
            not_after <= threshold
        }
        None => true,
    }
}

fn directory_url(production: bool) -> String {
    if production {
        LetsEncrypt::Production.url()
    } else {
        LetsEncrypt::Staging.url()
    }
    .to_owned()
}

/// Restore the ACME account from `acme_account_path` if present, otherwise register
/// a new account and persist its credentials for reuse across restarts.
async fn load_or_create_account(config: &Config) -> anyhow::Result<Account> {
    let account_path = config
        .acme_account_path
        .as_deref()
        .unwrap_or(ACCOUNT_PATH_DEFAULT);

    if let Ok(bytes) = tokio::fs::read(account_path).await {
        match serde_json::from_slice::<AccountCredentials>(&bytes) {
            Ok(creds) => match Account::builder()?.from_credentials(creds).await {
                Ok(account) => {
                    log_info("restored existing ACME account from disk");
                    return Ok(account);
                }
                Err(e) => log_warn(&format!(
                    "stored account credentials unusable ({e}); registering a new account"
                )),
            },
            Err(e) => log_warn(&format!(
                "could not parse {account_path} ({e}); registering a new account"
            )),
        }
    }

    let contact: Vec<String> = config
        .acme_contact_email
        .iter()
        .map(|e| format!("mailto:{e}"))
        .collect();
    let contact_refs: Vec<&str> = contact.iter().map(|s| s.as_str()).collect();

    let (account, credentials) = Account::builder()?
        .create(
            &NewAccount {
                contact: &contact_refs,
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            directory_url(config.acme_production),
            None,
        )
        .await?;

    if let Some(parent) = Path::new(account_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
    }
    tokio::fs::write(account_path, serde_json::to_vec_pretty(&credentials)?).await?;
    log_ok("registered new ACME account and saved credentials");
    Ok(account)
}

async fn write_file(path: &str, data: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }
    tokio::fs::write(path, data).await?;
    Ok(())
}

/// Ensure a valid certificate exists at the configured paths, obtaining one via
/// ACME (HTTP-01) when missing or near expiry. Returns `Ok(true)` if a new cert was
/// written, `Ok(false)` if the existing cert was still fresh (no API call made).
pub async fn ensure_certificate(config: &Config, store: &ChallengeStore) -> anyhow::Result<bool> {
    let cert_path = config
        .ssl_cert_path
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("ssl_cert_path is required when acme_enabled is true"))?;
    let key_path = config
        .ssl_key_path
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("ssl_key_path is required when acme_enabled is true"))?;
    if config.acme_domains.is_empty() {
        return Err(anyhow::anyhow!(
            "acme_domains must list at least one domain when acme_enabled is true"
        ));
    }

    if !cert_needs_renewal(cert_path, config.acme_renewal_days).await {
        log_info("existing certificate is still valid; skipping ACME request");
        return Ok(false);
    }

    log_info(&format!(
        "requesting certificate for [{}] via {}",
        config.acme_domains.join(", "),
        if config.acme_production { "production" } else { "staging" },
    ));

    let account = load_or_create_account(config).await?;
    let identifiers: Vec<Identifier> = config
        .acme_domains
        .iter()
        .map(|d| Identifier::Dns(d.clone()))
        .collect();
    let mut order = account
        .new_order(&NewOrder::new(identifiers.as_slice()))
        .await?;

    {
        let mut authorizations = order.authorizations();
        while let Some(result) = authorizations.next().await {
            let mut authz = result?;
            match authz.status {
                AuthorizationStatus::Pending => {}
                AuthorizationStatus::Valid => continue,
                other => {
                    return Err(anyhow::anyhow!(
                        "unexpected authorization status: {other:?}"
                    ));
                }
            }
            let mut challenge = authz.challenge(ChallengeType::Http01).ok_or_else(|| {
                anyhow::anyhow!("the ACME server did not offer an HTTP-01 challenge")
            })?;
            let token = challenge.token.clone();
            let key_auth = challenge.key_authorization().as_str().to_string();
            store.write().await.insert(token, key_auth);
            challenge.set_ready().await?;
        }
    }

    let status = order.poll_ready(&RetryPolicy::default()).await?;
    if status != OrderStatus::Ready {
        store.write().await.clear();
        return Err(anyhow::anyhow!(
            "ACME order did not become ready (status: {status:?})"
        ));
    }

    let key_pem = order.finalize().await?;
    let chain_pem = order.poll_certificate(&RetryPolicy::default()).await?;

    write_file(cert_path, chain_pem.as_bytes()).await?;
    write_file(key_path, key_pem.as_bytes()).await?;
    store.write().await.clear();

    log_ok(&format!("certificate written to {cert_path} and {key_path}"));
    Ok(true)
}

/// Periodically renew the certificate and hot-reload it into the running HTTPS
/// server without a restart.
pub async fn renewal_loop(config: Arc<Config>, store: ChallengeStore, rustls_config: RustlsConfig) {
    let cert_path = config.ssl_cert_path.clone();
    let key_path = config.ssl_key_path.clone();
    loop {
        tokio::time::sleep(Duration::from_secs(12 * 3600)).await;
        match ensure_certificate(&config, &store).await {
            Ok(true) => match (cert_path.as_deref(), key_path.as_deref()) {
                (Some(c), Some(k)) => match crate::utils::build_tls_config(c, k).await {
                    Ok(tls) => {
                        rustls_config.reload_from_config(tls);
                        log_ok("reloaded renewed certificate into the live server");
                    }
                    Err(e) => log_err(&format!("failed to reload renewed certificate: {e}")),
                },
                _ => {}
            },
            Ok(false) => {}
            Err(e) => log_err(&format!("renewal attempt failed: {e}")),
        }
    }
}
