use axum::{response::Html, routing::get, Router};
use tower_http::services::ServeDir;
use serde::{Deserialize, Serialize};
use toml;
use std::fs;
use std::path::Path;
use std::env;
use std::io::{self};
use std::collections::HashMap;
use rand::seq::SliceRandom;
use solarized::{
    print_colored, print_fancy, clear,
    VIOLET, BLUE, CYAN, GREEN, YELLOW, ORANGE, RED, MAGENTA,
    WHITE,
    BOLD, UNDERLINED, ITALIC,
    PrintMode::NewLine,
};

static IMAGE_DATA: &[u8] = include_bytes!("thing.png");

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    ip: String,
    port: u16,
    routes: HashMap<String, Vec<String>>,
}

fn read_config() -> Option<Config> {
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

fn read_media_files(dir: &str) -> std::io::Result<Vec<String>> {
    let paths = fs::read_dir(dir)?;
    let mut files = Vec::new();
    for path in paths {
        let path = path?.path();
        if path.is_file() {
            if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
                files.push(file_name.to_string());
            }
        }
    }
    Ok(files)
}

fn is_video_file(file_name: &str) -> bool {
    file_name.ends_with(".mp4") || file_name.ends_with(".webm") || file_name.ends_with(".ogg")
}

fn get_video_mime_type(file_name: &str) -> &str {
    if file_name.ends_with(".mp4") {
        "mp4"
    } else if file_name.ends_with(".webm") {
        "webm"
    } else if file_name.ends_with(".ogg") {
        "ogg"
    } else {
        "unknown"
    }
}

async fn render_html_with_media(file_path: &str, media_dir: &str, media_route: &str) -> Html<String> {
    let mut content = fs::read_to_string(file_path)
        .unwrap_or_else(|_| "<h1>Error loading page</h1>".to_string());
    if let Some(end_body_index) = content.find("\n</body>") {
        let mut media_files = read_media_files(media_dir)
            .unwrap_or_else(|_| vec![]);
        let mut rng = rand::thread_rng();
        media_files.shuffle(&mut rng);
        let media_tags = media_files.into_iter().map(|file| {
            if is_video_file(&file) {
                format!("
                    <video controls><source src='/static/{}/{}' type='video/{}'></video>
                    {}", media_route, file, get_video_mime_type(&file), file)
            } else {
                format!("<img src='/static/{}/{}'>", media_route, file)
            }
        }).collect::<Vec<_>>().join("\n");
        content.insert_str(end_body_index, &media_tags);
    }
    Html(content)
}

async fn render_html(file_path: &str) -> Html<String> {
    let content = fs::read_to_string(file_path)
        .unwrap_or_else(|_| "<h1>Error loading page</h1>".to_string());
    Html(content)
}

async fn root() -> Html<String> {
    render_html("static/home.html").await
}

fn app(config: &Config) -> Router {
    let mut router = Router::new()
        .route("/", get(root));
    for (path, settings) in &config.routes {
        if let Some(file_path) = settings.get(0) {
            let media_route = path.trim_start_matches('/');
            if let Some(media_dir) = settings.get(1) {
                let file_clone = file_path.clone();
                let media_dir_clone = media_dir.clone();
                let media_route_clone = media_route.to_string();
                router = router.route(path, get(move || {
                    let file = file_clone.clone();
                    let media = media_dir_clone.clone();
                    let route = media_route_clone.clone();
                    async move {
                        render_html_with_media(&file, &media, &route).await
                    }
                }));
                let serve_dir = ServeDir::new(media_dir);
                router = router.nest_service(&format!("/static/{}", media_route), serve_dir);
            } else {
                let file_clone = file_path.clone();
                router = router.route(path, get(move || {
                    async move {
                        render_html(&file_clone).await
                    }
                }));
            }
        }
    }
    router
}

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
    let config_option = read_config();
    if let Some(config) = config_option {
        print_fancy(&[
            ("config.yml ", CYAN, vec![]),
            ("found", GREEN, vec![]),
        ], NewLine);
        print_fancy(&[
            ("\nAddress : Port\n", CYAN, vec![BOLD, ITALIC, UNDERLINED]),
            (&format!("{}", config.ip), BLUE, vec![]),
            (":", CYAN, vec![BOLD]),
            (&format!("{}\n", config.port), VIOLET, vec![]),
            (&format!("http://{}:{}", config.ip, config.port), CYAN, vec![BOLD, ITALIC, UNDERLINED]),
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
        let address = format!("{}:{}", config.ip, config.port);
        axum::Server::bind(&address.parse().unwrap())
            .serve(app(&config).into_make_service())
            .await
            .unwrap();
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
            let example_config = r#"ip = "127.0.0.1"
port = 12345

[routes]
"/something" = ["static/home.html"]
"/stuff" = ["static/stuff.html", "static/media"]
"#;
            let example_home = r#"<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>guacamole</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    something
    <a href="/stuff">stuff</a>
</body>
</html>
"#;
            let example_stuff = r#"<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>guacamole</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <style>
    img, video {
        max-width: 100%;
        height: auto;
        display: block;
        margin: 0 auto;
    }
    </style>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <div class="container">
        <h1>Welcome to the stuff page.</h1>
        <p>This page shows media files.</p>
    </div>
</body>
</html>
"#;
            match fs::write("config.toml", example_config) {
                Ok(_) => {
                    print_fancy(&[
                        ("Example config.toml file has been created.", CYAN, vec![]),
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
                            ("static folder created ", CYAN, vec![]),
                            ("successfully", GREEN, vec![]),
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
                            ("media folder created ", CYAN, vec![]),
                            ("successfully", GREEN, vec![]),
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
            match fs::write("static/home.html", example_home) {
                Ok(_) => {
                    print_fancy(&[
                        ("Example ", CYAN, vec![]),
                        ("home.html", VIOLET, vec![]),
                        (" file has been ", CYAN, vec![]),
                        ("created.", GREEN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        (&format!("{}", e), CYAN, vec![]),
                    ], NewLine);
                }
            }
            match fs::write("static/stuff.html", example_stuff) {
                Ok(_) => {
                    print_fancy(&[
                        ("Example ", CYAN, vec![]),
                        ("stuff.html", VIOLET, vec![]),
                        (" file has been ", CYAN, vec![]),
                        ("created.", GREEN, vec![]),
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
                        ("Image saved to ", CYAN, vec![]),
                        (&format!("{}", image_path), VIOLET, vec![]),
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
                ("Files created in ", CYAN, vec![]),
                (&format!("{}", path.display()), VIOLET, vec![]),
            ], NewLine);
        }
    }
}
