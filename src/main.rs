mod config;
mod routes;
mod utils;
mod media;
mod constants;

use crate::config::read_config;
use crate::routes::app;
use crate::constants::*;

use std::fs;
use std::env;
use std::path::Path;
use std::io::{self};
use axum_server::{self, tls_rustls::RustlsConfig};
use solarized::{
    print_colored, print_fancy, clear,
    VIOLET, BLUE, CYAN, GREEN, YELLOW, ORANGE, RED, MAGENTA,
    WHITE,
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
            ("There is a hardcoded path which mounts static/home.html to /\n", CYAN, vec![]),
            ("All other paths are read from config.toml\n", CYAN, vec![]),
            ("If config.toml does not exist, an example project structure can be created.\n", CYAN, vec![]),
            ("The config.toml file should contain something similar to the following.\n", CYAN, vec![]),

            ("\nip", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("\"0.0.0.0\"\n", CYAN, vec![]),

            ("port", BLUE, vec![]),
            (" = ", WHITE, vec![]),
            ("12345\n\n", CYAN, vec![]),

            ("[routes]\n", ORANGE, vec![]),

            ("\"/something\"", BLUE, vec![]),
            (" = [", WHITE, vec![]),
            ("\"static/home.html\"", CYAN, vec![]),
            ("]\n", WHITE, vec![]),

            ("\"/stuff\"", BLUE, vec![]),
            (" = [", WHITE, vec![]),
            ("\"static/stuff.html\"", CYAN, vec![]),
            (", ", WHITE, vec![]),
            ("\"static/media\"", CYAN, vec![]),
            ("]", WHITE, vec![]),
        ], NewLine);
        return;
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
        print_fancy(&[
            ("\nAddress : Port\n", CYAN, vec![BOLD, ITALIC, UNDERLINED]),
            (&format!("{}", config.ip), BLUE, vec![]),
            (":", CYAN, vec![BOLD]),
            (&format!("{}\n", config.port), VIOLET, vec![]),
            (&format!("http://{}:{}", config.ip, config.port), GREEN, vec![BOLD, ITALIC, UNDERLINED]),
        ], NewLine);
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
            (&format!("{}", path.display()), VIOLET, vec![]),
        ], NewLine);
        /*
        let router = app(&config);
        let address = format!("{}:{}", config.ip, config.port);
        let listener = tokio::net::TcpListener::bind(&address).await.unwrap();
        axum::serve(listener, router).await.unwrap();
        */

        let config = read_config().expect("Failed to read configuration");
        let app = app(&config);
        if config.ssl_enabled {
            let ssl_config = RustlsConfig::from_pem_file(
                config.ssl_cert_path.expect("SSL cert path is required"),
                config.ssl_key_path.expect("SSL key path is required"),
            )
                .await
                .expect("Failed to configure SSL");
            let addr = format!("{}:{}", config.ip, config.ssl_port);
            axum_server::bind_rustls(addr.parse().unwrap(), ssl_config)
                .serve(app.into_make_service())
                .await
                .expect("Failed to start SSL server");
            } else {
                let addr = format!("{}:{}", config.ip, config.port);
                let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
                axum::serve(listener, app).await.unwrap();
        }

    } else {
        print_fancy(&[
            ("Failed to read configuration\n", ORANGE, vec![]),
            ("Example environment can be created in the current active directory.\n", CYAN, vec![]),
            ("Would you like to create an example environment?\n", CYAN, vec![]),
            ("(", VIOLET, vec![]),
            ("y", BLUE, vec![]),
            ("/", VIOLET, vec![]),
            ("n", RED, vec![]),
            (")", VIOLET, vec![]),
        ], NewLine);
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim().to_lowercase();
        if input == "y" || input == "yes" {
            clear();
            match fs::write("config.toml", EXAMPLE_CONFIG) {
                Ok(_) => {
                    print_fancy(&[
                        ("Example ", CYAN, vec![]),
                        ("config.toml", VIOLET, vec![]),
                        (" file has been ", CYAN, vec![]),
                        ("created", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Failed to create example config.toml file: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
            let templates = Path::new("static");
            if !templates.exists() {
                match fs::create_dir_all(&templates) {
                    Ok(_) => {
                        print_fancy(&[
                            ("The ", CYAN, vec![]),
                            ("static", VIOLET, vec![]),
                            (" folder has been ", CYAN, vec![]),
                            ("created", GREEN, vec![]),
                            (".", CYAN, vec![]),
                        ], NewLine);
                    }
                    Err(e) => println!("Error creating static: {:?}", e),
                }
            } else {
                print_fancy(&[
                    ("static folder exists", ORANGE, vec![]),
                ], NewLine);
            }
            let media = Path::new("static/media");
            if !media.exists() {
                match fs::create_dir_all(&media) {
                    Ok(_) => {
                        print_fancy(&[
                            ("The ", CYAN, vec![]),
                            ("static/media", VIOLET, vec![]),
                            (" folder has been ", CYAN, vec![]),
                            ("created", GREEN, vec![]),
                            (".", CYAN, vec![]),
                        ], NewLine);
                    }
                    Err(e) => {
                        print_fancy(&[
                            ("Error creating media: ", ORANGE, vec![]),
                            (&format!("{}", e), RED, vec![]),
                        ], NewLine);
                    }
                }
            } else {
                println!("media folder exists");
            }
            match fs::write("static/home.html", EXAMPLE_HOME) {
                Ok(_) => {
                    print_fancy(&[
                        ("Example ", CYAN, vec![]),
                        ("home.html", VIOLET, vec![]),
                        (" file has been ", CYAN, vec![]),
                        ("created", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        (&format!("{}", e), CYAN, vec![]),
                    ], NewLine);
                }
            }
            match fs::write("static/stuff.html", EXAMPLE_STUFF) {
                Ok(_) => {
                    print_fancy(&[
                        ("Example ", CYAN, vec![]),
                        ("stuff.html", VIOLET, vec![]),
                        (" file has been ", CYAN, vec![]),
                        ("created", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Failed to create example ", ORANGE, vec![]),
                        ("stuff.html", VIOLET, vec![]),
                        (" file: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
            match fs::write("static/pdf.html", EXAMPLE_PDF) {
                Ok(_) => {
                    print_fancy(&[
                        ("Example ", CYAN, vec![]),
                        ("pdf.html", VIOLET, vec![]),
                        (" file has been ", CYAN, vec![]),
                        ("created", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Failed to create example ", ORANGE, vec![]),
                        ("pdf.html", VIOLET, vec![]),
                        (" file: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
            match fs::write("static/error.html", EXAMPLE_ERROR) {
                Ok(_) => {
                    print_fancy(&[
                        ("Example ", CYAN, vec![]),
                        ("stuff.html", VIOLET, vec![]),
                        (" file has been ", CYAN, vec![]),
                        ("created", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Failed to create example ", ORANGE, vec![]),
                        ("stuff.html", VIOLET, vec![]),
                        (" file: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
            let image_path = "static/media/qrcode.png";
            match std::fs::write(image_path, IMAGE_DATA) {
                Ok(_) => {
                    print_fancy(&[
                        ("Image ", CYAN, vec![]),
                        (&format!("{}", image_path), VIOLET, vec![]),
                        (" has been ", CYAN, vec![]),
                        ("saved", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Failed to write image: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
            let video_path = "static/media/dancingcrab.webm";
            match std::fs::write(video_path, VIDEO_DATA) {
                Ok(_) => {
                    print_fancy(&[
                        ("Video ", CYAN, vec![]),
                        (&format!("{}", image_path), VIOLET, vec![]),
                        (" has been ", CYAN, vec![]),
                        ("saved", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Failed to write video: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
            let pdf_path = "static/documents/asdf.pdf";
            let pdf_dir = Path::new("static/documents");
            if !pdf_dir.exists() {
                match fs::create_dir_all(pdf_dir) {
                    Ok(_) => {
                        print_fancy(&[
                            ("The ", CYAN, vec![]),
                            ("static/documents", VIOLET, vec![]),
                            (" folder has been ", CYAN, vec![]),
                            ("created", GREEN, vec![]),
                            (".", CYAN, vec![]),
                        ], NewLine);
                    }
                    Err(e) => println!("Error creating static/documents: {:?}", e),
                }
            } else {
                print_fancy(&[
                    ("static/documents folder exists", ORANGE, vec![]),
                ], NewLine);
            }
            match std::fs::write(pdf_path, PDF_DATA) {
                Ok(_) => {
                    print_fancy(&[
                        ("Document ", CYAN, vec![]),
                        (&format!("{}", pdf_path), VIOLET, vec![]),
                        (" has been ", CYAN, vec![]),
                        ("saved", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Failed to write image: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
            let path = env::current_dir().expect("asdf");
            print_fancy(&[
                ("Setup in ", CYAN, vec![]),
                (&format!("{}", path.display()), VIOLET, vec![]),
                (" is ", CYAN, vec![]),
                ("complete", GREEN, vec![]),
                (".", CYAN, vec![]),
            ], NewLine);
        }
    }
}
