use std::fs;
use axum::{
    extract::DefaultBodyLimit,
    routing::{
        get, post, get_service
    },
    http::StatusCode, response::{
        Html, IntoResponse
    },
    Router
};
use tower_http::services::ServeDir;
use serde_json::Value;
use crate::config::Config;
use crate::media::{render_html, render_html_with_media};
use crate::upload::upload;
use solarized::{
    print_fancy,
    VIOLET, CYAN, RED, ORANGE,
    BOLD,
    PrintMode::NewLine,
};

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

pub fn parse_upload_limit(limit_val: &Option<Value>) -> Result<usize, &'static str> {
    match limit_val {
        Some(Value::String(s)) if s == "disabled" => Err("disabled"),
        Some(Value::String(s)) => s.parse::<usize>().map_err(|_| "default"),
        Some(Value::Number(n)) if n.is_u64() => Ok(n.as_u64().unwrap() as usize),
        _ => Err("default"),
    }
}

pub fn app(config: &Config) -> Router {
    let mut router = Router::new()
        .merge(routes_static())
        .route("/favicon.ico", get_service(ServeDir::new("./static")))
        .fallback(get(not_found));
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
                router = router
                    .nest_service(&format!("/static/{}", media_route), serve_dir);
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
    let something = config.upload_storage_limit;
    match parse_upload_limit(&config.upload_size_limit) {
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
