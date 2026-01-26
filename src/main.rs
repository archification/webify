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
mod interaction;
mod commands; // Module declaration for the new commands.rs file

use crate::config::read_config;
use crate::generate::*;
use crate::help::print_help;
use crate::out::setup;
use crate::routes::app;
use crate::forum::{init_db, ForumDb};
use crate::interaction::Room;

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
use rustls::crypto::ring;
use tokio::sync::broadcast;
use chrono::Utc;
use std::collections::HashMap;

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
    pub interaction: crate::interaction::InteractionState,
}

#[tokio::main]
async fn main() {
    let _ = ring::default_provider().install_default();
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
        
        // Initialize Interaction State
        let mut interaction = crate::interaction::InteractionState::new();

        // 1. Register Commands
        // This calls the registry in src/commands.rs to load all available commands
        crate::commands::register_all(&mut interaction);

        // 2. Initialize Permanent Rooms from config
        if let Some(permanent_rooms) = &config_arc.permanent_rooms {
            let mut rooms = interaction.rooms.write().await;
            for room_config in permanent_rooms {
                // Generate a simple ID from the name (slugify)
                let room_id = room_config.name.trim().to_lowercase().replace(" ", "-");
                let (tx, _rx) = broadcast::channel(100);
                
                let room = Room {
                    id: room_id.clone(),
                    label: room_config.name.clone(),
                    tx,
                    created_at: Utc::now(),
                    users: HashMap::new(),
                    max_controllers: room_config.max_controllers,
                    max_doers: room_config.max_doers,
                    current_color: "#808080".to_string(),
                    password: room_config.password.clone(),
                };
                
                rooms.insert(room_id, room);
            }
        }

        let state = Arc::new(AppState {
            config: config_arc.clone(),
            forum_db,
            tera,
            interaction,
        });
        let app = app(state.clone()).await;
        if state.config.ssl_enabled {
            let ssladdr = format_address(
                state.config.scope.as_str(), 
                state.config.ip.as_str(), 
                state.config.ssl_port
            );
            let ssl_config = RustlsConfig::from_pem_file(
                state.config.ssl_cert_path.clone().expect("SSL cert path is required"),
                state.config.ssl_key_path.clone().expect("SSL key path is required"),
            )
            .await
            .expect("Failed to configure SSL");
            let dual_server = axum_server_dual_protocol::bind_dual_protocol(
                ssladdr.parse().unwrap(), 
                ssl_config
            )
            .set_upgrade(true)
            .serve(app.clone().into_make_service_with_connect_info::<SocketAddr>());
            if state.config.port == state.config.ssl_port {
                dual_server.await.expect("Dual protocol server failed");
            } else {
                tokio::spawn(async move {
                    dual_server.await.expect("SSL server failed");
                });
                let addr = format_address(
                    state.config.scope.as_str(), 
                    state.config.ip.as_str(), 
                    state.config.port
                );
                let listener = tokio::net::TcpListener::bind(&addr).await.expect("Failed to bind HTTP port");
                axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
                    .await
                    .expect("HTTP server failed");
            }
        } else {
            let addr = format_address(
                state.config.scope.as_str(), 
                state.config.ip.as_str(), 
                state.config.port
            );
            let listener = tokio::net::TcpListener::bind(&addr).await.expect("Failed to bind HTTP port");
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
                .await
                .expect("HTTP server failed");
        }
    } else {
        generate_files();
    }
}
