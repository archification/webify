use axum::{routing::get, Router};
use tower_http::services::ServeDir;
use crate::config::Config;
use crate::media::{render_html, render_html_with_media};
use axum::{response::Html, /*routing::get, Router*/};

async fn root() -> Html<String> {
    render_html("static/home.html").await
}

pub fn app(config: &Config) -> Router {
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
