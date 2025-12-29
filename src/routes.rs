use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use std::io::SeekFrom;
use axum_extra::extract::Host;
use axum::{
    extract::{Path, DefaultBodyLimit, Query, Request, ConnectInfo},
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
use crate::php::handle_php;
//use crate::forum::{ForumDb, list_posts, new_post_form, create_post};
use crate::forum::*;
use solarized::{
    print_fancy,
    VIOLET, CYAN, RED, ORANGE,
    BOLD,
    PrintMode::NewLine,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::net::SocketAddr;
use tower::Service;

async fn not_found() -> impl IntoResponse {
    let file_path = "static/error.html";
    let custom_404_html = fs::read_to_string(file_path).await.unwrap_or_else(|_| {
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

#[derive(serde::Deserialize)]
pub struct LiveQuery {
    pub offset: Option<u64>,
}

fn routes_static() -> Router {
    Router::new().nest_service("/static", get_service(ServeDir::new("static")))
}

fn routes_uploads() -> Router {
    Router::new().nest_service("/uploads", get_service(ServeDir::new("uploads")))
}

pub async fn app(config: &Config, db: ForumDb) -> Router {
    let mut site_routers = HashMap::new();
    let whitelists = Arc::new(config.whitelists.clone());
    for (domain, routes) in &config.sites {
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
        for (path, settings) in routes {
            match settings.as_slice() {
                [template_path, mode] if mode == "forum" => {
                    let forum_state = Arc::new(ForumState {
                        db: db.clone(),
                        template_path: template_path.clone(),
                        base_path: path.clone(),
                    });
                    let forum_routes = Router::new()
                        .route("/", get(list_posts))
                        .route("/new", get(new_post_form))
                        .route("/create", post(create_post))
                        .route("/register", get(register_form).post(register))
                        .route("/login", get(login_form).post(login))
                        .route("/logout", get(logout))
                        .with_state(forum_state);
                    router = router.nest(path, forum_routes);
                }
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
                [file_path, watch_file, mode] if mode == "live" => {
                    let file_clone = file_path.clone();
                    let watch_clone = watch_file.clone();
                    let path_clone = path.clone();
                    router = router.route(&path_clone, get(move || {
                        let f = file_clone.clone();
                        async move { render_html(&f).await }
                    }));
                    let content_path = format!("{}/live_content", path_clone.trim_end_matches('/'));
                    router = router.route(&content_path, get(move |query: Query<LiveQuery>| {
                        let w = watch_clone.clone();
                        async move {
                            let mut file = match tokio::fs::File::open(&w).await {
                                Ok(f) => f,
                                Err(_) => return Html(format!("<p>Error opening log: {}</p>", w)),
                            };
                            let file_len = file.metadata().await.unwrap().len();
                            let start_offset = match query.offset {
                                Some(o) if o <= file_len => o,
                                _ => if file_len > 10_000 { file_len - 10_000 } else { 0 },
                            };
                            let _ = file.seek(SeekFrom::Start(start_offset)).await;
                            let mut buffer = Vec::new();
                            let _ = file.read_to_end(&mut buffer).await;
                            let content = String::from_utf8_lossy(&buffer);
                            Html(format!(
                                r#"{}<input id="current-offset" name="offset" value="{}" hx-swap-oob="true" type="hidden">"#,
                                content, file_len
                            ))
                        }
                    }));
                }
                [dir_path, fpm_addr, mode] if mode == "php" => {
                    let path_clone = path.clone();
                    router = router.route(
                        &format!("{}/{{*path}}", path_clone.trim_end_matches('/')),
                        get(handle_php).post(handle_php)
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
        let storage_limit = config.upload_storage_limit;
        match parse_upload_limit(&config.upload_size_limit).await {
            Ok(num) => {
                router = router.route(
                    "/upload",
                    post(move |multipart| upload(multipart, storage_limit))
                    .layer(DefaultBodyLimit::max(num))
                );
            },
            Err("disabled") => {
                router = router.route(
                    "/upload",
                    post(move |multipart| upload(multipart, storage_limit))
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
                    ("2GB", VIOLET, vec![]),
                ], NewLine);
                let default_limit = 2 * 1000 * 1000 * 1000;
                router = router.route(
                    "/upload",
                    post(move |multipart| upload(multipart, storage_limit))
                    .layer(DefaultBodyLimit::max(default_limit))
                );
            }
        }
        site_routers.insert(domain.clone(), router);
    }
    let site_routers = Arc::new(site_routers);
Router::new().fallback(move |host: Host, ConnectInfo(addr): ConnectInfo<SocketAddr>, req: Request| {
        let routers = Arc::clone(&site_routers);
        let whitelist_map = Arc::clone(&whitelists);
        async move {
            let hostname = host.0.split(':').next().unwrap_or("").to_string();
            let client_ip = addr.ip().to_string();
            if let Some(ips) = whitelist_map.get(&hostname).or_else(|| whitelist_map.get("default")) {
                if !ips.is_empty() && !ips.contains(&client_ip) {
                    return (StatusCode::FORBIDDEN, "Access Denied").into_response();
                }
            }
            if let Some(router) = routers.get(&hostname) {
                router.clone().call(req).await.unwrap().into_response()
            } else if let Some(default) = routers.get("default") {
                default.clone().call(req).await.unwrap().into_response()
            } else {
                not_found().await.into_response()
            }
        }
    })
}

async fn render_post(Path(post_name): Path<String>) -> Html<String> {
    let file_path = format!("static/posts/{}.md", post_name);
    let markdown_content = match fs::read_to_string(&file_path).await {
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
