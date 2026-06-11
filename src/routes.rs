use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use std::io::SeekFrom;
use axum::{
    extract::{Path, DefaultBodyLimit, Query, Request, ConnectInfo, Form},
    response::{Html, IntoResponse, Redirect},
    routing::{
        get, post, get_service
    },
    http::{
        header, HeaderMap, StatusCode
    },
    Router
};
use axum::extract::State;
use tower_http::services::{ServeDir, ServeFile};
use pulldown_cmark::{Parser, Options, html};
use crate::media::render_html_with_media;
use crate::upload::upload;
use crate::limits::parse_upload_limit;
use crate::thumbnail::generate_thumbnail;
use crate::slideshow::handle_slideshow;
use crate::slideshow::SlideQuery;
use crate::php::handle_php;
use crate::interaction;
use crate::stream;
use sqlx;
use crate::AppState;
use crate::forum::*;
use crate::auth_guard;
use crate::admin;
use solarized::{
    print_fancy,
    VIOLET, CYAN, RED, ORANGE,
    BOLD,
    PrintMode::NewLine,
};
use std::collections::HashMap;
use axum_extra::extract::CookieJar;
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

pub async fn app(state: Arc<AppState>) -> Router {
    let mut site_routers = HashMap::new();
    let whitelists = Arc::new(state.config.whitelists.clone());
    for (domain, routes) in &state.config.sites {
        let mut router = Router::new()
            .route("/thumbnail/{*path}", get(generate_thumbnail))
            .route("/blog", get(crate::blog::blog_index))
            .route("/blog/new", get(crate::blog::new_post_form))
            .route("/blog/edit/{slug}", get(crate::blog::edit_post_form))
            .route("/blog/create", post(crate::blog::create_post)
                .layer(DefaultBodyLimit::max(64 * 1024 * 1024)))
            .route("/blog/update", post(crate::blog::update_post)
                .layer(DefaultBodyLimit::max(64 * 1024 * 1024)))
            .route("/blog/upload-image", post(crate::blog::upload_image)
                .layer(DefaultBodyLimit::max(64 * 1024 * 1024)))
            .route("/blog/{post_name}", get(render_post))
            .nest_service("/static", ServeDir::new("static"))
            .nest_service("/templates", ServeDir::new("templates"))
            .nest_service("/uploads", ServeDir::new("uploads"))
            .route("/favicon.ico", get_service(ServeFile::new("static/favicon.ico")))
            .nest_service("/css", ServeDir::new("css"))
            .nest_service("/styles", ServeDir::new("styles"))
            .nest_service("/js", ServeDir::new("js"))
            .nest_service("/scripts", ServeDir::new("scripts"))
            .nest_service("/images", ServeDir::new("images"))
            .route("/auth/login", get(auth_guard::guard_login))
            .route("/auth/google", get(auth_guard::guard_google))
            .route("/auth/callback", get(auth_guard::guard_callback))
            .route("/auth/logout", get(auth_guard::guard_logout))
            .route("/interaction", get(render_interaction_page))
            .route("/interaction/create", post(interaction::create_room))
            .route("/interaction/join", post(interaction::join_room))
            .route("/interaction/list/{role}", get(interaction::list_rooms))
            .route("/interaction/upload/{room_id}",
                post(interaction::upload_file)
                    .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
            )
            .route("/ws/interaction/{room_id}", get(interaction::ws_handler))
            .fallback(get(not_found));
        let mut config_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (path, settings) in routes {
            config_paths.insert(path.clone());
            match settings.as_slice() {
                [template_path, mode] if mode == "forum" => {
                    let forum_routes = Router::new()
                        .route("/", get(board_index))
                        .route("/c/{category_id}", get(view_category)) // Shows threads in a category
                        .route("/new/{category_id}", get(new_post_form))
//                        .route("/new", get(new_post_form))
                        .route("/create", post(create_post))
                        .route("/thread/{id}", get(view_thread))
                        .route("/thread/{id}/reply", post(post_reply))
                        .route("/thread/{id}/lock", post(toggle_lock))
                        .route("/register", get(register_form).post(register))
                        .route("/login", get(login_form).post(login))
                        .route("/logout", get(logout))
                        .route("/verify", get(verify_email))
                        .route("/auth/google", get(login_google))
                        .route("/auth/google/callback", get(callback_google))
                        .route("/admin", get(admin_panel))
                        .route("/admin/delete-post/{id}", post(admin_delete_post))
                        .route("/admin/delete-reply/{id}", post(admin_delete_reply))
                        .route("/admin/edit-post/{id}", get(admin_edit_post_form).post(admin_edit_post))
                        .route("/admin/ban/{username}", post(admin_ban_user))
                        .route("/admin/unban/{username}", post(admin_unban_user))
                        .route("/admin/set-role/{username}", post(admin_set_role))
                        .route("/admin/add-category", post(admin_add_category))
                        .route("/admin/delete-category/{id}", post(admin_delete_category));
                    router = router.nest(path, forum_routes);
                }
                [settings_type, slides_dir] if settings_type == "slideshow" => {
                    let slides_dir_clone = slides_dir.clone();
                    let autoplay = state.config.slideshow_autoplay;
                    let timer = state.config.slideshow_timer;
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
                [dir_path, mode] if mode == "static" => {
                    println!("The correct mode is selected for wiki");
                    let serve_dir = ServeDir::new(dir_path);
                    let path_no_slash = path.trim_end_matches('/').to_string();
                    let path_slash = format!("{}/", path_no_slash);
                    let redirect_target = path_slash.clone();
                    router = router.route(&path_no_slash, get(move || async move {
                        Redirect::permanent(&redirect_target)
                    }));
                    router = router.nest_service(&path_slash, serve_dir);
                }
                [file_path, watch_file, mode] if mode == "live" => {
                    let file_clone = file_path.clone();
                    let watch_clone = watch_file.clone();
                    let path_clone = path.clone();
                    router = router.route(&path_clone, get(move |s: State<Arc<AppState>>| {
                        let f = file_clone.clone();
                        async move { render_tera_template(s, f, tera::Context::new()).await }
                    }));
                    let content_path = format!("{}/live_content", path_clone.trim_end_matches('/'));
                    router = router.route(&content_path, get(move |State(_): State<Arc<AppState>>, query: Query<LiveQuery>| {
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
                    let file_clone = file_path.clone();
                    let media_dir_clone = media_dir.clone();
                    let media_route = path.trim_start_matches('/').to_string();
                    let sort_method = settings.get(2).map(|s| s.to_string());
                    router = router.route(path, get(move |s: State<Arc<AppState>>| {
                        let f = file_clone.clone();
                        let m = media_dir_clone.clone();
                        let r = media_route.clone();
                        let sort = sort_method.clone();
                        async move {
                            render_html_with_media(&s.tera, &f, &m, &r, sort.as_deref()).await
                        }
                    }));
                    let serve_dir = ServeDir::new(media_dir);
                    let static_route = path.trim_start_matches('/');
                    router = router
                        .nest_service(&format!("/static/{static_route}"), serve_dir);
                }
                [file_path] => {
                    let file_clone = file_path.clone();
                    router = router.route(path, get(move |s: State<Arc<AppState>>| {
                        let f = file_clone.clone();
                        async move { render_tera_template(s, f, tera::Context::new()).await }
                    }));
                }
                _ => {}
            }
        }
        // Add admin dashboard routes for any configured dashboard whose domain matches this site
        for dashboard in &state.config.admin_dashboards {
            let site_match = dashboard.domain.is_empty()
                || domain.eq_ignore_ascii_case(&dashboard.domain);
            if !site_match {
                continue;
            }
            let dp = dashboard.path.trim_end_matches('/').to_string();
            if !config_paths.contains(&dp) {
                // GET main page
                let dp_get = dp.clone();
                router = router.route(&dp, get(move |
                    s: State<Arc<AppState>>,
                    j: CookieJar,
                    h: HeaderMap,
                    q: Query<HashMap<String, String>>,
                | {
                    let path = dp_get.clone();
                    async move { admin::dashboard_page(s, j, h, q, path).await }
                }));

                // POST add rule
                let dp_add_rule = dp.clone();
                router = router.route(&format!("{}/rules", dp), post(move |
                    s: State<Arc<AppState>>,
                    j: CookieJar,
                    h: HeaderMap,
                    f: Form<admin::AddRuleForm>,
                | {
                    let path = dp_add_rule.clone();
                    async move { admin::add_rule(s, j, h, f, path).await }
                }));

                // POST delete rule
                let dp_del_rule = dp.clone();
                router = router.route(&format!("{}/rules/delete", dp), post(move |
                    s: State<Arc<AppState>>,
                    j: CookieJar,
                    h: HeaderMap,
                    f: Form<admin::DeleteRuleForm>,
                | {
                    let path = dp_del_rule.clone();
                    async move { admin::delete_rule(s, j, h, f, path).await }
                }));

                // POST add editor
                let dp_add_ed = dp.clone();
                router = router.route(&format!("{}/editors", dp), post(move |
                    s: State<Arc<AppState>>,
                    j: CookieJar,
                    h: HeaderMap,
                    f: Form<admin::AddEditorForm>,
                | {
                    let path = dp_add_ed.clone();
                    async move { admin::add_editor(s, j, h, f, path).await }
                }));

                // POST revoke editor
                let dp_rev_ed = dp.clone();
                router = router.route(&format!("{}/editors/revoke", dp), post(move |
                    s: State<Arc<AppState>>,
                    j: CookieJar,
                    h: HeaderMap,
                    f: Form<admin::RevokeEditorForm>,
                | {
                    let path = dp_rev_ed.clone();
                    async move { admin::revoke_editor(s, j, h, f, path).await }
                }));
            }
        }

        // Add streaming routes after config routes to avoid conflicts with any config-defined paths
        if !config_paths.contains("/live") {
            router = router.route("/live", get(stream::live_handler));
        }
        if !config_paths.contains("/live/whip") {
            router = router.route("/live/whip", post(stream::whip_ingest));
        }
        if !config_paths.contains("/live/whip/{session_id}") {
            router = router.route("/live/whip/{session_id}", axum::routing::patch(stream::whip_patch).delete(stream::whip_delete));
        }
        if !config_paths.contains("/watch") {
            router = router.route("/watch", get(stream::watch_handler));
        }
        if !config_paths.contains("/watch/status") {
            router = router.route("/watch/status", get(stream::watch_status));
        }
        if !config_paths.contains("/watch/whep") {
            router = router.route("/watch/whep", post(stream::whep_handler));
        }
        if !config_paths.contains("/watch/whep/{session_id}") {
            router = router.route("/watch/whep/{session_id}", axum::routing::patch(stream::whep_patch));
        }
        let storage_limit = state.config.upload_storage_limit;
        match parse_upload_limit(&state.config.upload_size_limit).await {
            Ok(num) => {
                router = router.route(
                    "/upload",
                    post(move |headers, multipart| upload(headers, multipart, storage_limit))
                    .layer(DefaultBodyLimit::max(num))
                );
            },
            Err("disabled") => {
                router = router.route(
                    "/upload",
                    post(move |headers, multipart| upload(headers, multipart, storage_limit))
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
                    post(move |headers, multipart| upload(headers, multipart, storage_limit))
                    .layer(DefaultBodyLimit::max(default_limit))
                );
            }
        }
        let final_site_router = router.with_state(state.clone());
        site_routers.insert(domain.clone(), final_site_router);
    }
    let site_routers_arc = Arc::new(site_routers);
    let guard_state = state.clone();
    // ACME HTTP-01 challenge responder. Registered on the outermost router so it
    // takes precedence over the fallback and bypasses the whitelist / auth-guard
    // checks below — it must be reachable over plain HTTP for cert issuance/renewal.
    let challenge_store = state.acme_challenges.clone();
Router::new()
    .route("/.well-known/acme-challenge/{token}", get(move |Path(token): Path<String>| {
        let store = challenge_store.clone();
        async move {
            match store.read().await.get(&token) {
                Some(key_auth) => (StatusCode::OK, key_auth.clone()).into_response(),
                None => (StatusCode::NOT_FOUND, "not found").into_response(),
            }
        }
    }))
    .fallback(move |headers: HeaderMap, ConnectInfo(addr): ConnectInfo<SocketAddr>, req: Request| {
        let routers = Arc::clone(&site_routers_arc);
        let whitelist_map = Arc::clone(&whitelists);
        let gs = guard_state.clone();
        async move {
            let hostname = headers
                .get(header::HOST)
                .and_then(|h| h.to_str().ok())
                .and_then(|h| h.split(':').next())
                .unwrap_or("")
                .to_string();
            let client_ip = addr.ip().to_string();
            if let Some(ips) = whitelist_map.get(&hostname).or_else(|| whitelist_map.get("default")) {
                if !ips.is_empty() && !ips.contains(&client_ip) {
                    return (StatusCode::FORBIDDEN, "Access Denied").into_response();
                }
            }
            // Auth guard: check email-based access control before routing
            let path = req.uri().path().to_string();
            if !path.starts_with("/auth/") {
                let cookie_header = req.headers()
                    .get(header::COOKIE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();

                // Check if this path belongs to a configured admin dashboard
                let matching_dashboard = gs.config.admin_dashboards.iter().find(|d| {
                    let site_match = d.domain.is_empty() || d.domain.eq_ignore_ascii_case(&hostname);
                    let prefix = d.path.trim_end_matches('/');
                    let pm = path == prefix || path.starts_with(&format!("{}/", prefix));
                    site_match && pm
                });

                if let Some(dashboard) = matching_dashboard {
                    let token = auth_guard::extract_cookie_value(&cookie_header, auth_guard::GUARD_COOKIE);
                    let email = match token {
                        Some(ref t) => auth_guard::validate_session(&gs.forum_db, t).await,
                        None => None,
                    };
                    let allowed = if let Some(ref e) = email {
                        let elc = e.to_ascii_lowercase();
                        let owner = dashboard.owners.iter().any(|o| o.to_ascii_lowercase() == elc);
                        if owner {
                            true
                        } else {
                            sqlx::query_scalar::<_, bool>(
                                "SELECT COUNT(*) > 0 FROM dashboard_editors WHERE email = ?",
                            )
                            .bind(&elc)
                            .fetch_one(&*gs.forum_db)
                            .await
                            .unwrap_or(false)
                        }
                    } else {
                        false
                    };
                    if !allowed {
                        let login_url = format!(
                            "/auth/login?next={}&host={}",
                            urlencoding::encode(&path),
                            urlencoding::encode(&hostname),
                        );
                        return Redirect::to(&login_url).into_response();
                    }
                } else {
                    // Regular path: check config auth_guards + DB access_rules
                    let config_guard = auth_guard::find_guard(&gs.config.auth_guards, &hostname, &path);
                    let has_db = {
                        let rules = gs.access_rules.read().await;
                        auth_guard::has_db_rule(&rules, &hostname, &path)
                    };
                    if config_guard.is_some() || has_db {
                        let token = auth_guard::extract_cookie_value(&cookie_header, auth_guard::GUARD_COOKIE);
                        let email = match token {
                            Some(ref t) => auth_guard::validate_session(&gs.forum_db, t).await,
                            None => None,
                        };
                        let allowed = match &email {
                            Some(e) => {
                                let rules = gs.access_rules.read().await;
                                config_guard.map(|g| auth_guard::email_allowed(g, e)).unwrap_or(false)
                                    || auth_guard::db_rule_allows(&rules, &hostname, &path, e)
                            }
                            None => false,
                        };
                        if !allowed {
                            let login_url = format!(
                                "/auth/login?next={}&host={}",
                                urlencoding::encode(&path),
                                urlencoding::encode(&hostname),
                            );
                            return Redirect::to(&login_url).into_response();
                        }
                    }
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

async fn render_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(post_name): Path<String>,
) -> impl IntoResponse {
    let file_path = format!("static/posts/{}.md", post_name);
    let markdown_content = match fs::read_to_string(&file_path).await {
        Ok(content) => content,
        Err(_) => return Html("Post not found".to_string()).into_response(),
    };
    let (front_matter, body) = crate::blog::parse_front_matter(&markdown_content);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(&body, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    let display_title = front_matter.title.unwrap_or_else(|| post_name.clone());
    let can_edit = crate::blog::viewer_can_edit(&state, &headers).await;
    let post_date_display = front_matter.date.as_deref().and_then(crate::blog::humanize_date);

    let mut context = tera::Context::new();
    context.insert("port", &state.config.port);
    context.insert("domain", &state.config.domain);
    context.insert("post_title", &display_title);
    context.insert("post_slug", &post_name);
    context.insert("post_image", &front_matter.image);
    context.insert("post_date", &front_matter.date);
    context.insert("post_date_display", &post_date_display);
    context.insert("can_edit", &can_edit);
    context.insert("post_content", &html_output);

    if let Ok(rendered) = state.tera.render("post.html", &context) {
        return Html(rendered).into_response();
    }

    // Fallback: plain solarized template if post.html is missing
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
    Html(template).into_response()
}

async fn render_interaction_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let context = tera::Context::new();
    match state.tera.render("interaction.html", &context) {
        Ok(html) => Html(html),
        Err(e) => Html(format!("Error rendering interaction: {}", e)),
    }
}

async fn render_tera_template(
    State(state): State<Arc<AppState>>, 
    template_path: String,
    mut context: tera::Context,
) -> impl IntoResponse {
    context.insert("port", &state.config.port);
    context.insert("domain", &state.config.domain);
    let template_name = template_path.trim_start_matches("static/");
    match state.tera.render(template_name, &context) {
        Ok(rendered) => Html(rendered).into_response(),
        Err(e) => {
            eprintln!("Tera error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
        }
    }
}
