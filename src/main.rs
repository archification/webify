mod config;
mod constants;
mod generate;
mod help;
mod limits;
mod media;
mod out;
mod routes;
mod upload;
mod utils;
mod slideshow;
mod thumbnail;
mod php;
mod forum;

use crate::config::read_config;
use crate::generate::*;
use crate::help::print_help;
use crate::out::setup;
use crate::routes::app;
use crate::forum::{init_db, ForumDb};

use axum_server::tls_rustls::RustlsConfig;
use axum_server_dual_protocol::ServerExt;
use tera::Tera;
use solarized::{
    BLUE, CYAN, GREEN, MAGENTA, ORANGE, RED, VIOLET, YELLOW,
    PrintMode::NewLine,
    clear,
    print_colored,
};
use std::env;
use std::sync::Arc;
use std::net::SocketAddr;
use webbrowser;

fn format_address(scope: &str, ip: &str, port: u16) -> String {
    let scope = scope.trim().to_lowercase();
    match scope.as_str() {
        "localhost" | "local" => format!("127.0.0.1:{}", &port),
        "lan" => format!("{}:{}", &ip, &port),
        "public" | "production" | "prod" => format!("[::]:{}", &port),
        _ => format!("127.0.0.1:{}", &port),
    }
}

pub struct AppState {
    pub config: Arc<crate::config::Config>,
    pub forum_db: crate::forum::ForumDb,
    pub tera: Tera,
}

#[tokio::main]
async fn main() {
    clear();
    let args: Vec<String> = env::args().collect();
    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_help(args[0].clone());
        return;
    };
    print_colored(
        &["R", "a", "i", "n", "b", "o", "w", "s"],
        &[VIOLET, BLUE, CYAN, GREEN, YELLOW, ORANGE, RED, MAGENTA],
        NewLine,
    );
    if let Some(config) = read_config() {
        setup().await;
        if config.browser && (config.scope == "localhost" || config.scope == "local") {
            let addr = if config.ssl_enabled {
                format!(
                    "https://{}",
                    format_address(config.scope.as_str(), &config.ip, config.ssl_port)
                )
            } else {
                format!(
                    "http://{}",
                    format_address(config.scope.as_str(), &config.ip, config.port)
                )
            };
            tokio::spawn(async move {
                if let Err(e) = webbrowser::open(&addr) {
                    print_colored(
                        &["Failed to open browser: ", &e.to_string()],
                        &[ORANGE, RED],
                        NewLine,
                    );
                }
            });
        }
        let mut tera = match Tera::new("static/**/*.html") {
            Ok(t) => t,
            Err(e) => {
                println!("Tera parsing error: {}", e);
                std::process::exit(1);
            }
        };
        tera.autoescape_on(vec![]);
        let config_arc = Arc::new(config);
        let forum_db: ForumDb = init_db().await;
        let state = Arc::new(AppState {
            config: config_arc.clone(),
            forum_db,
            tera,
        });
        let app = app(state.clone()).await;
        if state.config.ssl_enabled {
            let ssladdr =
                format_address(state.config.scope.as_str(), state.config.ip.as_str(), state.config.ssl_port);
            let ssl_config = RustlsConfig::from_pem_file(
                state.config
                    .ssl_cert_path
                    .clone()
                    .expect("SSL cert path is required"),
                state.config
                    .ssl_key_path
                    .clone()
                    .expect("SSL key path is required"),
            )
            .await
            .expect("Failed to configure SSL");
            let server =
                axum_server_dual_protocol::bind_dual_protocol(ssladdr.parse().unwrap(), ssl_config)
                    .set_upgrade(true)
                    .serve(app.clone().into_make_service_with_connect_info::<SocketAddr>());
            tokio::spawn(async move {
                let result: Result<(), std::io::Error> = server.await;
                result.expect("SSL server failed");
            });
        }
        let addr = format_address(state.config.scope.as_str(), state.config.ip.as_str(), state.config.port);
        let listener = tokio::net::TcpListener::bind(&addr).await.expect("Failed to bind HTTP port");
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .expect("HTTP server failed");
    } else {
        generate_files();
    }
}
