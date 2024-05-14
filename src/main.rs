mod config;
mod routes;
mod utils;
mod media;
mod constants;
mod generate;
mod archive;
mod upload;

use crate::config::read_config;
use crate::routes::{app, parse_upload_limit};
use crate::generate::*;
use crate::archive::add_dir_to_zip;

use webify::run;

use std::env;
use axum_server::{tls_rustls::RustlsConfig};
use solarized::{
    print_colored, print_fancy, clear,
    VIOLET, BLUE, CYAN, GREEN, YELLOW, ORANGE, RED, MAGENTA,
    WHITE, GREY,
    BOLD, UNDERLINED, ITALIC,
    PrintMode::NewLine,
};

#[tokio::main]
async fn main() {
    clear();
    let args: Vec<String> = env::args().collect();
    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_fancy(&[
            ("This program is designed to be a modular web service.\n", CYAN, vec![]),
            ("All paths and routes are read from config.toml\n", CYAN, vec![]),
            ("If config.toml does not exist, an example project structure can be created.\n\n", CYAN, vec![]),
            ("There is builtin archive support. Here's an example:\n", CYAN, vec![]),
            ("webify -b <path/to/source/directory> <path/to/destination/zip>\n\n", VIOLET, vec![]),
            ("The config.toml file should contain something similar to the following.\n", CYAN, vec![]),
//base web config
            ("\nip", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("\"127.0.0.1\"\n", CYAN, vec![]),
            ("port", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("12345\n", CYAN, vec![]),
//ssl config
            ("ssl_enabled", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("false\n", CYAN, vec![]),
            ("ssl_port", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("44444\n", CYAN, vec![]),
            ("ssl_cert_path", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("\"pems/cert.pem\"\n", CYAN, vec![]),
            ("ssl_key_path", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("\"pems/key.pem\"\n", CYAN, vec![]),
//todo config
            ("todo_enabled", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("true\n", CYAN, vec![]),
            ("todo_ip", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("\"127.0.0.1\"\n", CYAN, vec![]),
            ("todo_port", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("11111\n", CYAN, vec![]),
//upload limit
            ("upload_size_limit ", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("2000000000", CYAN, vec![]),
            (" # 2 GB (2 * 1000 * 1000 * 1000)\n\n", GREY, vec![]),
//default routes
            ("[routes]\n", ORANGE, vec![]),
//home route
            ("\"/\"", BLUE, vec![]),
            (" = [", WHITE, vec![]),
            ("\"static/home.html\"", CYAN, vec![]),
            ("]\n", WHITE, vec![]),
//stuff route
            ("\"/stuff\"", BLUE, vec![]),
            (" = [", WHITE, vec![]),
            ("\"static/stuff.html\"", CYAN, vec![]),
            (", ", WHITE, vec![]),
            ("\"static/media\"", CYAN, vec![]),
            ("]\n", WHITE, vec![]),
//pdf route
            ("\"/pdf\"", BLUE, vec![]),
            (" = [", WHITE, vec![]),
            ("\"static/pdf.html\"", CYAN, vec![]),
            (", ", WHITE, vec![]),
            ("\"static/documents\"", CYAN, vec![]),
            ("]\n", WHITE, vec![]),
//downloads route
            ("\"/downloads\"", BLUE, vec![]),
            (" = [", WHITE, vec![]),
            ("\"static/downloads.html\"", CYAN, vec![]),
            (", ", WHITE, vec![]),
            ("\"static/files\"", CYAN, vec![]),
            ("]\n", WHITE, vec![]),
        ], NewLine);
        return;
    } else if args.contains(&"-b".to_string()) || args.contains(&"--backup".to_string()) {
        let index = args.iter().position(|x| x == "-b" || x == "--backup").unwrap_or_else(|| args.len());
        if args.len() <= index + 2 {
            print_fancy(&[
                ("Error: ", RED, vec![]),
                ("Missing arguments.\n", ORANGE, vec![]),
                ("Usage: ", CYAN, vec![]),
                ("--backup <source_directory_path> <destination_zip_path>", VIOLET, vec![]),
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
    }
    print_colored(
        &["R", "a", "i", "n", "b", "o", "w", "s"],
        &[VIOLET, BLUE, CYAN, GREEN, YELLOW, ORANGE, RED, MAGENTA],
        NewLine
    );
    let config_option = read_config(); if let Some(config) = config_option {
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
                (&format!("https://{}:{}\n", config.ip, config.ssl_port), GREEN, vec![BOLD, ITALIC, UNDERLINED]),
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
                (&format!("http://{}:{}", config.ip, config.port), GREEN, vec![BOLD, ITALIC, UNDERLINED]),
            ], NewLine);
        }
        if config.todo_enabled {
            print_fancy(&[
                ("\nTodo", GREEN, vec![]),
                (" is ", CYAN, vec![]),
                ("Enabled", GREEN, vec![]),
                ("\nAddress : Port\n", CYAN, vec![BOLD, ITALIC, UNDERLINED]),
                (&format!("{}", config.todo_ip), BLUE, vec![]),
                (":", CYAN, vec![BOLD]),
                (&format!("{}\n", config.todo_port), VIOLET, vec![]),
                (&format!("http://{}:{}\n", config.todo_ip, config.todo_port), GREEN, vec![BOLD, ITALIC, UNDERLINED]),
            ], NewLine);
        } else {
            print_fancy(&[
                ("\nTodo", YELLOW, vec![]),
                (" is ", CYAN, vec![]),
                ("NOT", RED, vec![BOLD, ITALIC]),
                (" Enabled", ORANGE, vec![]),
            ], NewLine);
        }
        match parse_upload_limit(&config.upload_size_limit) {
            Ok(num) => {
                print_fancy(&[
                    ("\nUpload limit size: ", CYAN, vec![]),
                    (&format!("{}", num), CYAN, vec![]),
                    ("\n", CYAN, vec![]),
                ], NewLine);
            },
            Err("disabled") => {
                print_fancy(&[
                    ("\nUpload limit size: ", CYAN, vec![]),
                    ("disabled", CYAN, vec![]),
                    ("\n", CYAN, vec![]),
                ], NewLine);
            },
            _ => {
                print_fancy(&[
                    ("\nUpload limit size: ", CYAN, vec![]),
                    ("null", CYAN, vec![]),
                    ("\n", CYAN, vec![]),
                ], NewLine);
            }
        }
        print_fancy(&[
            ("\nHardcoded routes:\n", CYAN, vec![BOLD, ITALIC, UNDERLINED]),
            ("/", BLUE, vec![]),
            (" -> ", CYAN, vec![]),
            ("root", VIOLET, vec![]),
        ], NewLine);
        print_fancy(&[
            ("\nConfigured routes:", CYAN, vec![BOLD, ITALIC, UNDERLINED]),
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
        if config.ssl_enabled {
            let app = app(&config);
            let ssl_config = RustlsConfig::from_pem_file(
                config.ssl_cert_path.expect("SSL cert path is required"),
                config.ssl_key_path.expect("SSL key path is required"),
            ).await.expect("Failed to configure SSL");
            let addr = format!("{}:{}", config.ip, config.ssl_port);
            let server = axum_server_dual_protocol::bind_dual_protocol(addr.parse().unwrap(), ssl_config)
                .serve(app.into_make_service());
            if config.todo_enabled {
                let todoaddr = format!("{}:{}", config.todo_ip, config.todo_port);
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
            } else {
                server.await.unwrap();
            }
        } else {
            let app = app(&config);
            let addr = format!("{}:{}", config.ip, config.port);
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
            let server = axum::serve(listener, app);
            if config.todo_enabled {
                let todoaddr = format!("{}:{}", config.todo_ip, config.todo_port);
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
            } else {
                if let Err(e) = server.await {
                    eprintln!("Server error: {:?}", e);
                }
            }
        }
    } else {
        generate_files();
    }
}
