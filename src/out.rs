use crate::config::read_config;
use crate::limits::parse_upload_limit;
use crate::format_address;
use std::env;
use solarized::{
    print_fancy,
    VIOLET, BLUE, CYAN, GREEN, YELLOW, ORANGE, RED, MAGENTA,
    BOLD, UNDERLINED, ITALIC,
    PrintMode::NewLine,
};

pub async fn setup() {
    let config_option = read_config(); if let Some(config) = config_option {
        let ssladdr = format_address(config.scope.as_str(), config.ip.as_str(), config.ssl_port);
        let addr = format_address(config.scope.as_str(), config.ip.as_str(), config.port);
        print_fancy(&[
            ("config.yml ", CYAN, vec![]),
            ("found", GREEN, vec![]),
        ], NewLine);
        if config.ssl_enabled {
            print_fancy(&[
                ("\nSSL", GREEN, vec![]),
                (" is ", CYAN, vec![]),
                ("Enabled\n", GREEN, vec![]),
                ("\nAddress : Port\n", CYAN, vec![BOLD, ITALIC, UNDERLINED]),
                (&format!("{}", config.ip), BLUE, vec![]),
                (":", CYAN, vec![BOLD]),
                (&format!("{}\n", config.ssl_port), VIOLET, vec![]),
                (&format!("https://{}\n", ssladdr), GREEN, vec![BOLD, ITALIC, UNDERLINED]),
            ], NewLine);
        } else {
            print_fancy(&[
                ("\nSSL", YELLOW, vec![]),
                (" is ", CYAN, vec![]),
                ("NOT", RED, vec![BOLD, ITALIC]),
                (" Enabled\n", ORANGE, vec![]),
                ("\nAddress : Port\n", CYAN, vec![BOLD, ITALIC, UNDERLINED]),
                (&format!("{}", config.ip), BLUE, vec![]),
                (":", CYAN, vec![BOLD]),
                (&format!("{}\n", config.port), VIOLET, vec![]),
                (&format!("http://{}", addr), GREEN, vec![BOLD, ITALIC, UNDERLINED]),
            ], NewLine);
        }
        match parse_upload_limit(&config.upload_size_limit).await {
            Ok(num) => {
                print_fancy(&[
                    ("Upload limit size: ", CYAN, vec![]),
                    (&format!("{}", num), CYAN, vec![]),
                ], NewLine);
            },
            Err("disabled") => {
                print_fancy(&[
                    ("Upload limit size: ", CYAN, vec![]),
                    ("disabled", CYAN, vec![]),
                ], NewLine);
            },
            _ => {
                print_fancy(&[
                    ("Upload limit size: ", CYAN, vec![]),
                    ("null", CYAN, vec![]),
                ], NewLine);
            }
        }
        match parse_upload_limit(&config.upload_storage_limit).await {
            Ok(num) => {
                print_fancy(&[
                    ("Upload limit storage: ", CYAN, vec![]),
                    (&format!("{}", num), CYAN, vec![]),
                    ("\n", CYAN, vec![]),
                ], NewLine);
            }
            Err("disabled") => {
                print_fancy(&[
                    ("Upload limit storage: ", CYAN, vec![]),
                    ("null", CYAN, vec![]),
                    ("\n", CYAN, vec![]),
                ], NewLine);
            }
            Err(err) => {
                eprintln!("Error parsing upload limit: {}", err);
            }
        }
        print_fancy(&[
            ("Configured routes:", CYAN, vec![BOLD, ITALIC, UNDERLINED]),
        ], NewLine);
        for (path, settings) in &config.routes {
            let file = settings.get(0)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "No file specified".to_string());
            let media_info = if settings.len() > 1 {
                format!("{}", settings[1])
            } else {
                "".to_string()
            };
            print_fancy(&[
                (&format!("{}", path), BLUE, vec![]),
                (" -> ", CYAN, vec![]),
                (&format!("{}", &file), VIOLET, vec![]),
                (" -> ", CYAN, vec![]),
                (&format!("{}", &media_info), MAGENTA, vec![]),
            ], NewLine);
        }
        let path = env::current_dir().expect("asdf");
        print_fancy(&[
            ("\nServer running in ", CYAN, vec![]),
            (&format!("{}\n", path.display()), VIOLET, vec![]),
        ], NewLine);
    }
}
