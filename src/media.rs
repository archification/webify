use axum::response::Html;
use tera::{Tera, Context};
use crate::utils::read_media_files;
use rand::{seq::SliceRandom, rng};
use solarized::{print_fancy, RED, BOLD, PrintMode::NewLine};

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
    let mut context = Context::new();
    context.insert("media_files", &media_files);
    context.insert("media_route", &media_route);
    context.insert("media_dir", &media_dir);
    let template_name = file_path.trim_start_matches("static/");
    match tera.render(template_name, &context) {
        Ok(rendered) => Html(rendered),
        Err(e) => {
            eprintln!("Tera render error: {}", e);
            return Html("<h1>Internal Server Error</h1>".to_string())
        }
    }
}
