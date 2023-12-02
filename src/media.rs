use axum::response::Html;
use std::fs;
use crate::utils::{read_media_files, is_video_file, is_audio_file, get_video_mime_type, get_audio_mime_type};
use rand::seq::SliceRandom;

pub async fn render_html_with_media(file_path: &str, media_dir: &str, media_route: &str) -> Html<String> {
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
            } else if is_audio_file(&file) {
                format!("
                    <audio controls><source src='/static/{}/{}' type='audio/{}'></audio>
                    ", media_route, file, get_audio_mime_type(&file))
            } else {
                format!("<img src='/static/{}/{}'>", media_route, file)
            }
        }).collect::<Vec<_>>().join("\n");
        content.insert_str(end_body_index, &media_tags);
    }
    Html(content)
}

pub async fn render_html(file_path: &str) -> Html<String> {
    let content = fs::read_to_string(file_path)
        .unwrap_or_else(|_| "<h1>Error loading page</h1>".to_string());
    Html(content)
}
