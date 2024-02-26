use axum::response::Html;
use std::fs;
use crate::utils::{
    read_media_files,
    is_image_file, is_video_file, is_audio_file, is_pdf_file,
    get_video_mime_type, get_audio_mime_type
};
use rand::seq::SliceRandom;
use solarized::{print_fancy, RED, BOLD, PrintMode::NewLine};

pub async fn render_error_page() -> Html<String> {
    match fs::read_to_string("static/error.html") {
        Ok(contents) => Html(contents),
        Err(_) => Html("<h1>Internal Server Error</h1>".to_string()),
    }
}

pub async fn render_html_with_media(file_path: &str, media_dir: &str, media_route: &str) -> Html<String> {
    let content = match fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            print_fancy(&[
                ("Error: Unable to read file: ", RED, vec![]),
                (file_path, RED, vec![BOLD]),
                (&format!(" - {}", e), RED, vec![])
            ], NewLine);
            return render_error_page().await;
        }
    };
    if let Some(end_body_index) = content.find("\n</body>") {
        let mut media_files = match read_media_files(media_dir) {
            Ok(files) => files,
            Err(_) => {
                print_fancy(&[
                    ("Error: Unable to read media directory: ", RED, vec![]),
                    (media_dir, RED, vec![BOLD]),
                ], NewLine);
                return Html(content);
            }
        };
        let mut rng = rand::thread_rng();
        media_files.shuffle(&mut rng);
        let media_tags = media_files.into_iter().map(|file| {
            if is_video_file(&file) {
                format!("<video controls><source src='/static/{}/{}' type='video/{}'></video>{}", media_route, file, get_video_mime_type(&file), file)
            } else if is_audio_file(&file) {
                format!("<audio controls><source src='/static/{}/{}' type='audio/{}'></audio>", media_route, file, get_audio_mime_type(&file))
            } else if is_pdf_file(&file) {
                format!("<iframe src='/static/{}/{}' width='100%' height='600px'></iframe>", media_route, file)
            } else if is_image_file(&file) {
                format!("<img src='/static/{}/{}'>", media_route, file)
            } else {
                format!("")
            }
        }).collect::<Vec<_>>().join("\n");
        let mut new_content = content.clone();
        new_content.insert_str(end_body_index, &media_tags);
        Html(new_content)
    } else {
        Html(content)
    }
}

pub async fn render_html(file_path: &str) -> Html<String> {
    let content = match fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            print_fancy(&[
                ("Error: Unable to read file: ", RED, vec![]),
                (file_path, RED, vec![BOLD]),
                (&format!(" - {}", e), RED, vec![])
            ], NewLine);
            return render_error_page().await;
        }
    };
    Html(content)
}
