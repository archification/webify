use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use toml;
use solarized::{
    print_fancy,
    ORANGE, RED,
    BOLD,
    PrintMode::NewLine,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub ip: String,
    pub port: u16,
    pub routes: HashMap<String, Vec<String>>,
}

pub fn read_config() -> Option<Config> {
    let contents = match fs::read_to_string("config.toml") {
        Ok(c) => c,
        Err(e) => {
            print_fancy(&[
                ("Error reading config file in read_config\n", ORANGE, vec![]),
                (&format!("{}", e), RED, vec![BOLD])
            ], NewLine);
            return None;
        }
    };
    match toml::from_str(&contents) {
        Ok(config) => Some(config),
        Err(e) => {
            print_fancy(&[
                ("Error parsing config file in read_config", ORANGE, vec![]),
                (&format!("{}", e), RED, vec![BOLD])
            ], NewLine);
            None
        }
    }
}
