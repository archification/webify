use axum::response::Html;
use tokio::fs;
use tera::{Tera, Context};
use crate::utils::{
    read_media_files,
    is_image_file, is_video_file,
    get_video_mime_type,
};
use rand::{seq::SliceRandom, rng};
use solarized::{print_fancy, RED, BOLD, PrintMode::NewLine};

pub async fn render_error_page() -> Html<String> {
    match fs::read_to_string("static/error.html").await {
        Ok(contents) => Html(contents),
        Err(_) => Html("<h1>Internal Server Error</h1>".to_string()),
    }
}

pub async fn render_html_with_media(
    tera: &Tera,
    file_path: &str,
    media_dir: &str,
    media_route: &str,
    sort_method: Option<&str>
) -> Html<String> {
    let mut media_files = match read_media_files(media_dir).await {
        Ok(files) => files,
        Err(_) => {
            print_fancy(&[
                ("Error: Unable to read media directory: ", RED, vec![]),
                (media_dir, RED, vec![BOLD]),
            ], NewLine);
            return Html("<h1>Error reading media directory</h1>".to_string());
        }
    };
    match sort_method {
        Some("random") => {
            media_files.shuffle(&mut rng());
        }
        Some("alphanumeric") => {
            media_files.sort();
        }
        _ => {
            media_files.sort();
        }
    }
    let media_tags = media_files.iter().map(|file| {
        if is_video_file(file) {
            format!("<video controls><source src='/static/{}/{}' type='video/{}'></video>", media_route, file, get_video_mime_type(file))
        } else if is_image_file(file) {
            format!("<img src='/static/{}/{}'>", media_route, file)
        } else {
            "".to_string()
        }
    }).collect::<Vec<String>>().join("\n");
    let mut context = Context::new();
    context.insert("media_tags", &media_tags);
    let template_name = file_path.trim_start_matches("static/");
    match tera.render(template_name, &context) {
        Ok(rendered) => Html(rendered),
        Err(e) => {
            eprintln!("Tera render error: {}", e);
            render_error_page().await
        }
    }
}

pub async fn render_html(tera: &Tera, file_path: &str) -> Html<String> {
    let context = Context::new();
    let template_name = file_path.trim_start_matches("static/");
    
    match tera.render(template_name, &context) {
        Ok(rendered) => Html(rendered),
        Err(_) => render_error_page().await,
    }
}
