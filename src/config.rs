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

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub scope: String,
    pub ip: String,
    pub port: u16,
    pub ssl_enabled: bool,
    pub ssl_port: u16,
    pub ssl_cert_path: Option<String>,
    pub ssl_key_path: Option<String>,
    pub upload_size_limit: Option<Value>,
    pub upload_storage_limit: Option<u64>,
    pub browser: bool,
    pub sites: HashMap<String, Vec<(String, Vec<String>)>>,
    pub whitelists: HashMap<String, Vec<String>>,
    pub slideshow_autoplay: bool,
    pub slideshow_timer: u64,
    pub domain: String,
    pub smtp_server: Option<String>,
    pub smtp_port: Option<u16>,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
    pub email_from: Option<String>,
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
    upload_size_limit: Option<Value>,
    upload_storage_limit: Option<u64>,
    browser: bool,
    slideshow_autoplay: bool,
    slideshow_timer: u64,
    domain: String,
    smtp_server: Option<String>,
    smtp_port: Option<u16>,
    smtp_username: Option<String>,
    smtp_password: Option<String>,
    email_from: Option<String>,
    #[serde(default)]
    routes: HashMap<String, RouteValue>,
    #[serde(default)]
    whitelist: HashMap<String, WhitelistValue>,
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
    let raw: RawConfig = toml::from_str(&contents).map_err(|e| {
        print_fancy(&[
            ("Error parsing config file: ", ORANGE, vec![]),
            (&format!("{}", e), RED, vec![BOLD])
        ], NewLine);
    }).ok()?;
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
                for (_, ips) in map { // Flattens sub-keys like "allowed_ips"
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
        upload_size_limit: raw.upload_size_limit,
        upload_storage_limit: raw.upload_storage_limit,
        browser: raw.browser,
        sites,
        whitelists,
        slideshow_autoplay: raw.slideshow_autoplay,
        slideshow_timer: raw.slideshow_timer,
        domain: raw.domain.trim().to_string(),
        smtp_server: raw.smtp_server,
        smtp_port: raw.smtp_port,
        smtp_username: raw.smtp_username,
        smtp_password: raw.smtp_password,
        email_from: raw.email_from,
    })
}
