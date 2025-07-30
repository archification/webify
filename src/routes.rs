use std::fs;
use axum::{
    extract::{Path, DefaultBodyLimit, Query},
    routing::{
        get, post, get_service
    },
    http::StatusCode, response::{
        Html, IntoResponse
    },
    Router
};
use tower_http::services::{ServeDir, ServeFile};
use pulldown_cmark::{Parser, Options, html};
use crate::config::Config;
use crate::media::{render_html, render_html_with_media};
use crate::upload::upload;
use crate::limits::parse_upload_limit;
use crate::thumbnail::generate_thumbnail;
use crate::slideshow::handle_slideshow;
use crate::slideshow::SlideQuery;
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

fn routes_uploads() -> Router {
    Router::new().nest_service("/uploads", get_service(ServeDir::new("uploads")))
}

pub async fn app(config: &Config) -> Router {
    let mut router = Router::new()
        .route("/thumbnail/{*path}", get(generate_thumbnail))
        .route("/blog/{post_name}", get(render_post))
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
                let autoplay = config.slideshow_autoplay;
                let timer = config.slideshow_timer;
                router = router.route(
                    path,
                    get(move |query: Query<SlideQuery>| {
                        let dir = slides_dir_clone.clone();
                        async move {
                            handle_slideshow(query, &dir, autoplay, timer).await
                        }
                    }),
                );
            }
            [file_path, media_dir, ..] => {
                let sort_method = settings.get(2).map(|s| s.as_str());
                let media_route = path.trim_start_matches('/');
                let file_clone = file_path.clone();
                let media_dir_clone = media_dir.clone();
                let media_route_clone = media_route.to_string();
                let sort_method_clone = sort_method.map(|s| s.to_string());
                router = router.route(path, get(move || {
                    let file = file_clone.clone();
                    let media = media_dir_clone.clone();
                    let route = media_route_clone.clone();
                    let sort = sort_method_clone.clone();
                    async move {
                        render_html_with_media(&file, &media, &route, sort.as_deref()).await
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

async fn render_post(Path(post_name): Path<String>) -> Html<String> {
    let file_path = format!("static/posts/{}.md", post_name);
    let markdown_content = match fs::read_to_string(&file_path) {
        Ok(content) => content,
        Err(_) => return Html("Post not found".to_string()),
    };
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(&markdown_content, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    let template = format!(r#"
<!doctype html>
<html>
<head>
    <title>{}</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css">
    <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/styles/base16/solarized-dark.min.css">
    <script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/highlight.min.js"></script>
    <script>hljs.highlightAll();</script>
</head>
<body>
    <div style="max-width: 800px; margin: 0 auto; padding: 20px;">
        <a href="/blog">← Back to All Posts</a>
        <hr>
        {}
    </div>
</body>
</html>
    "#, post_name, html_output);
    Html(template)
}
