use std::fs;
use axum::{
    extract::{DefaultBodyLimit, Query},
    routing::{
        get, post, get_service
    },
    http::StatusCode, response::{
        Html, IntoResponse
    },
    Router
};
use tower_http::services::{ServeDir, ServeFile};
use serde::Deserialize;
use crate::config::Config;
use crate::media::{render_html, render_html_with_media};
use crate::upload::upload;
use crate::limits::parse_upload_limit;
use solarized::{
    print_fancy,
    VIOLET, CYAN, RED, ORANGE,
    BOLD,
    PrintMode::NewLine,
};

#[derive(Debug, Deserialize)]
struct SlideQuery {
    current: Option<usize>,
}

async fn read_slides(slides_dir: &str) -> Result<Vec<String>, std::io::Error> {
    let mut dir = tokio::fs::read_dir(slides_dir).await?;
    let mut entries = Vec::new();
    while let Some(entry) = dir.next_entry().await? {
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("txt")
            && let Some(filename) = path.file_name().and_then(|f| f.to_str())
        {
            entries.push(filename.to_owned());
        }
    }
    entries.sort_by(|a, b| {
        let a_num = a.trim_end_matches(".txt").parse::<usize>().unwrap_or(0);
        let b_num = b.trim_end_matches(".txt").parse::<usize>().unwrap_or(0);
        a_num.cmp(&b_num)
    });
    Ok(entries)
}

async fn parse_slide_file(slide_path: &str) -> Result<(String, String), std::io::Error> {
    let content = tokio::fs::read_to_string(slide_path).await?;
    let mut lines = content.lines();
    let image_path = lines.next().unwrap_or_default().to_string();
    let text = lines.collect::<Vec<_>>().join("\n");
    Ok((image_path, text))
}

async fn handle_slideshow(
    Query(query): Query<SlideQuery>,
    slides_dir: &str,
) -> Html<String> {
    let slides = match read_slides(slides_dir).await {
        Ok(s) => s,
        Err(e) => return Html(format!("Error reading slides directory: {}", &e)),
    };
    if slides.is_empty() {
        return Html("No slides found".to_string());
    }
    let current = query.current.unwrap_or(0);
    let total = slides.len();
    let current_index = current % total;
    let slide_file = format!("{}/{}", slides_dir, slides[current_index]);
    let (image_path, content) = match parse_slide_file(&slide_file).await {
        Ok((ip, c)) => (ip, c),
        Err(e) => return Html(format!("Error reading slide file: {}", &e)),
    };
    let next_index = (current_index + 1) % total;
    let html = format!(
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <div id="countdown">Redirecting in 30...</div>
            <button onclick="skipRedirect()" id="skipBtn">Skip Now</button>

            <script>
            let countdownTimer;
            let seconds = 30;

            // Start the countdown automatically when page loads
            window.onload = function() {{
                countdownTimer = setInterval(() => {{
                    seconds--;
                    document.getElementById("countdown").textContent = `Redirecting in ${{seconds}}...`;
                    if (seconds <= 0) {{
                        clearInterval(countdownTimer);
                        performRedirect();
                    }}
                }}, 1000);
            }};

            function skipRedirect() {{
                clearInterval(countdownTimer);
                performRedirect();
            }}

            function performRedirect() {{
                window.location.href = `?current={next_index}`;
            }}
            </script>

            <style>
            #skipBtn {{
                padding: 8px 16px;
                background: #28a745;
                color: white;
                border: none;
                border-radius: 4px;
                cursor: pointer;
                margin-top: 10px;
            }}

            #skipBtn:hover {{
                background: #218838;
            }}

            #countdown {{
                margin: 20px 0;
                font-size: 1.2em;
            }}
            </style>

            <link rel="stylesheet" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css">
        </head>
        <body>
            <img src="/static/{image_path}" style="max-width: 100%; height: auto;">
            <pre>{content}</pre>
        </body>
        </html>
        "#
    );

    Html(html)
}

async fn not_found() -> impl IntoResponse {
    let file_path = "static/error.html";
    let custom_404_html = fs::read_to_string(file_path).unwrap_or_else(|_| {
    String::from(r#"
<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>guacamole</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>ERROR</h1>
    <p>You shouldn't be here. Please go away.</p>
</body>
</html>
"#)
    });
    (StatusCode::NOT_FOUND, Html(custom_404_html))
}

fn routes_static() -> Router {
    Router::new().nest_service("/static", get_service(ServeDir::new("static")))
}

fn routes_uploads() -> Router {
    Router::new().nest_service("/uploads", get_service(ServeDir::new("uploads")))
}

pub async fn app(config: &Config) -> Router {
    let mut router = Router::new()
        .merge(routes_static())
        .merge(routes_uploads())
        .route("/favicon.ico", get_service(ServeFile::new("static/favicon.ico")))
        .nest_service("/styles", ServeDir::new("styles"))
        .nest_service("/scripts", ServeDir::new("scripts"))
        .nest_service("/images", ServeDir::new("images"))
        .fallback(get(not_found));
    for (path, settings) in &config.routes {
        match settings.as_slice() {
            [settings_type, slides_dir] if settings_type == "slideshow" => {
                let slides_dir_clone = slides_dir.clone();
                router = router.route(
                    path,
                    get(move |query: Query<SlideQuery>| {
                        let dir = slides_dir_clone.clone();
                        async move { handle_slideshow(query, &dir).await }
                    }),
                );
            }
            [file_path, media_dir] => {
                let media_route = path.trim_start_matches('/');
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
                router = router
                    .nest_service(&format!("/static/{media_route}"), serve_dir);
            }
            [file_path] => {
                let file_clone = file_path.clone();
                router = router.route(path, get(move || {
                    async move {
                        render_html(&file_clone).await
                    }
                }));
            }
            _ => {}
        }
    }
    let something = config.upload_storage_limit;
    match parse_upload_limit(&config.upload_size_limit).await {
        Ok(num) => {
            router = router.route(
                "/upload",
                post(move |multipart| upload(multipart, something))
                .layer(DefaultBodyLimit::max(num))
            );
        },
        Err("disabled") => {
            router = router.route(
                "/upload",
                post(move |multipart| upload(multipart, something))
                .layer(DefaultBodyLimit::disable())
            );
        },
        _ => {
            print_fancy(&[
                ("Error", RED, vec![BOLD]),
                (": ", CYAN, vec![]),
                ("config.upload_size_limit", VIOLET, vec![]),
                (" is ", CYAN, vec![]),
                ("null", ORANGE, vec![]),
                (": ", CYAN, vec![]),
                ("Defaulting to ", CYAN, vec![]),
                ("2 * 1000 * 1000 * 1000 || 2GB", VIOLET, vec![]),
            ], NewLine);
            let default_limit = 2 * 1000 * 1000 * 1000;
            router = router.route(
                "/upload",
                post(move |multipart| upload(multipart, something))
                .layer(DefaultBodyLimit::max(default_limit))
            );
        }
    }
    router
}
