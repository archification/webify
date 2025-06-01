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
    pub routes: HashMap<String, Vec<String>>,
    pub slideshow_autoplay: bool,
    pub slideshow_timer: u64,
}

pub fn read_config() -> Option<Config> {
    let contents = match fs::read_to_string("config.toml") {
        Ok(c) => c,
        Err(e) => {
            print_fancy(&[
                ("\nError reading config file in read_config\n", ORANGE, vec![]),
                (&format!("{}", e), RED, vec![BOLD])
            ], NewLine);
            return None;
        }
    };
    match toml::from_str(&contents) {
        Ok(config) => Some(config),
        Err(e) => {
            print_fancy(&[
                ("Error parsing config file in read_config\n\n", ORANGE, vec![]),
                (&format!("{}", e), RED, vec![BOLD])
            ], NewLine);
            None
        }
    }
}
