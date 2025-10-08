use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    pub routes: Vec<(String, Vec<String>)>,
    pub slideshow_autoplay: bool,
    pub slideshow_timer: u64,
}

#[derive(Debug, Deserialize)]
struct PartialConfig {
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
}

pub fn read_config() -> Option<Config> {
    let contents = match fs::read_to_string("config.toml") {
        Ok(c) => c,
        Err(e) => {
            print_fancy(&[
                ("\nError reading config file in read_config\n", ORANGE, vec![]),
                (&format!("{}", &e), RED, vec![BOLD])
            ], NewLine);
            return None;
        }
    };
    let partial_config: PartialConfig = match toml::from_str(&contents) {
        Ok(config) => config,
        Err(e) => {
            print_fancy(&[
                ("Error parsing config file in read_config\n\n", ORANGE, vec![]),
                (&format!("{}", &e), RED, vec![BOLD])
            ], NewLine);
            return None;
        }
    };
    let mut routes = Vec::new();
    let mut in_routes_section = false;
    for line in contents.lines() {
        let trimmed_line = line.trim();
        if trimmed_line == "[routes]" {
            in_routes_section = true;
            continue;
        }
        if trimmed_line.starts_with('[') && trimmed_line != "[routes]" {
            in_routes_section = false;
            continue;
        }
        if in_routes_section && trimmed_line.contains('=') && let Some((key, value)) = trimmed_line.split_once('=') {
            let path = key.trim().trim_matches('"').to_string();
            let settings_str = value.trim().trim_start_matches('[').trim_end_matches(']');
            let settings = settings_str.split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .collect();
            routes.push((path, settings));
        }
    }
    Some(Config {
        scope: partial_config.scope,
        ip: partial_config.ip,
        port: partial_config.port,
        ssl_enabled: partial_config.ssl_enabled,
        ssl_port: partial_config.ssl_port,
        ssl_cert_path: partial_config.ssl_cert_path,
        ssl_key_path: partial_config.ssl_key_path,
        upload_size_limit: partial_config.upload_size_limit,
        upload_storage_limit: partial_config.upload_storage_limit,
        browser: partial_config.browser,
        routes, // Use the ordered routes.
        slideshow_autoplay: partial_config.slideshow_autoplay,
        slideshow_timer: partial_config.slideshow_timer,
    })
}
