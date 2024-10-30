mod config;
mod routes;
mod utils;
mod media;
mod constants;
mod generate;
mod archive;
mod upload;
mod help;
mod out;

use crate::config::read_config;
use crate::routes::app;
use crate::generate::*;
use crate::archive::add_dir_to_zip;
use crate::help::print_help;
use crate::out::setup;

use webify::run;

use std::env;
use axum_server::tls_rustls::RustlsConfig;
use axum_server_dual_protocol::ServerExt;
use webbrowser;
use solarized::{
    print_colored, print_fancy, clear,
    VIOLET, BLUE, CYAN, GREEN, YELLOW, ORANGE, RED, MAGENTA,
    PrintMode::NewLine,
};

fn browser(protocol: &str, addr: String) {
    let url = format!("{}://{}", protocol, addr);
    if webbrowser::open(&url).is_ok() {
        print_fancy(&[
            ("Opened ", GREEN, vec![]),
            ("browser to ", CYAN, vec![]),
            (&format!("{}", url), VIOLET, vec![]),
        ], NewLine);
    } else {
        print_colored(
            &["Failed to open browser"],
            &[RED],
            NewLine
        );
    }
}

fn format_address(scope: &str, ip: &str, port: u16) -> String {
    match scope {
        "localhost" | "local" => format!("127.0.0.1:{}", port),
        "lan" => format!("{}:{}", ip, port),
        "public" | "production" | "prod" => format!("0.0.0.0:{}", port),
        _ => format!("127.0.0.1:{}", port),
    }
}

#[tokio::main]
async fn main() {
    clear();
    let args: Vec<String> = env::args().collect();
    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_help(args[0].clone());
    } else if args.contains(&"-b".to_string()) || args.contains(&"--backup".to_string()) {
        let index = args.iter().position(|x| x == "-b" || x == "--backup").unwrap_or_else(|| args.len());
        if args.len() <= index + 2 {
            print_fancy(&[
                ("Error: ", RED, vec![]),
                ("Missing arguments.\n", ORANGE, vec![]),
                ("Usage: ", CYAN, vec![]),
                (&format!("{}", args[0]), VIOLET, vec![]),
                ("--backup", VIOLET, vec![]),
                (" <", CYAN, vec![]),
                ("source_directory_path", VIOLET, vec![]),
                ("> <", CYAN, vec![]),
                ("destination_zip_path", VIOLET, vec![]),
                (">", CYAN, vec![]),
            ], NewLine);
            std::process::exit(1);
        } else {
            let source_directory = &args[index + 1];
            let destination_zip = &args[index + 2];
            match add_dir_to_zip(source_directory, destination_zip) {
                Ok(_) => {
                    print_fancy(&[
                        ("Zip ", CYAN, vec![]),
                        ("Success", GREEN, vec![]),
                    ], NewLine);
                    std::process::exit(0);
                },
                Err(e) => {
                    print_fancy(&[
                        ("Zip ", CYAN, vec![]),
                        ("Failure", RED, vec![]),
                        (": ", CYAN, vec![]),
                        (&format!("{}", e), CYAN, vec![]),
                    ], NewLine);
                    std::process::exit(0);
                },
            }
        }
    };
    print_colored(
        &["R", "a", "i", "n", "b", "o", "w", "s"],
        &[VIOLET, BLUE, CYAN, GREEN, YELLOW, ORANGE, RED, MAGENTA],
        NewLine
    );
    let config_option = read_config();
    if let Some(config) = config_option {
        setup().await;
        match (config.ssl_enabled, config.todo_enabled) {
            (true, true) => {
                let ssladdr = format_address(config.scope.as_str(), config.ip.as_str(), config.ssl_port);
                let app = app(&config);
                let ssl_config = RustlsConfig::from_pem_file(
                    config.ssl_cert_path.clone().expect("SSL cert path is required"),
                    config.ssl_key_path.clone().expect("SSL key path is required"),
                ).await.expect("Failed to configure SSL");
                let server = axum_server_dual_protocol::bind_dual_protocol(ssladdr.parse().unwrap(), ssl_config)
                    .set_upgrade(true)
                    .serve(app.await.into_make_service());
                let todoaddr = format_address(config.todo_scope.as_str(), config.todo_ip.as_str(), config.todo_port);
                let todo_task = tokio::spawn(async {
                    run(todoaddr).await;
                });
                let server_task = tokio::spawn(async {
                    server.await.unwrap();
                });
                let (todo_result, server_result) = tokio::join!(todo_task, server_task);
                if let Err(e) = todo_result {
                    eprintln!("Error from todo task: {:?}", e);
                }
                if let Err(e) = server_result {
                    eprintln!("Error from server task: {:?}", e);
                }
                if config.browser {
                    browser("https", ssladdr);
                }
            },
            (true, false) => {
                let ssladdr = format_address(config.scope.as_str(), config.ip.as_str(), config.ssl_port);
                let app = app(&config);
                let ssl_config = RustlsConfig::from_pem_file(
                    config.ssl_cert_path.clone().expect("SSL cert path is required"),
                    config.ssl_key_path.clone().expect("SSL key path is required"),
                ).await.expect("Failed to configure SSL");
                let server = axum_server_dual_protocol::bind_dual_protocol(ssladdr.parse().unwrap(), ssl_config)
                    .set_upgrade(true)
                    .serve(app.await.into_make_service());
                let server_task = tokio::spawn(async {
                    server.await.unwrap();
                });
                let (server_result,) = tokio::join!(server_task,);
                if let Err(e) = server_result {
                    eprintln!("Error from server task: {:?}", e);
                }
                if config.browser {
                    browser("https", ssladdr);
                }
            },
            (false, true) => {
                let app = app(&config);
                let addr = format_address(config.scope.as_str(), config.ip.as_str(), config.port);
                let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
                let server = axum::serve(listener, app.await);
                let todoaddr = format_address(config.todo_scope.as_str(), config.todo_ip.as_str(), config.todo_port);
                let todo_task = tokio::spawn(async {
                    run(todoaddr).await;
                });
                let server_task = tokio::spawn(async {
                    server.await.unwrap();
                });
                let (todo_result, server_result) = tokio::join!(todo_task, server_task);
                if let Err(e) = todo_result {
                    eprintln!("Error from todo task: {:?}", e);
                }
                if let Err(e) = server_result {
                    eprintln!("Error from server task: {:?}", e);
                }
                if config.browser {
                    browser("http", addr);
                }
            },
            (false, false) => {
                let app = app(&config);
                let addr = format_address(config.scope.as_str(), config.ip.as_str(), config.port);
                let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
                let server = axum::serve(listener, app.await);
                let server_task = tokio::spawn(async {
                    server.await.unwrap();
                });
                let (server_result,) = tokio::join!(server_task,);
                if let Err(e) = server_result {
                    eprintln!("Error from server task: {:?}", e);
                }
                if config.browser {
                    browser("http", addr);
                }
            },
        }
    } else {
        generate_files();
    }
}
