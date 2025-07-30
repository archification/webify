use axum::response::Html;
use std::fs;
use crate::utils::{
    read_media_files,
    is_image_file, is_video_file, is_audio_file, is_pdf_file, is_zip_file,
    is_markdown_file,
    get_video_mime_type, get_audio_mime_type
};
use rand::{seq::SliceRandom, rng};
use solarized::{print_fancy, RED, BOLD, PrintMode::NewLine};

pub async fn render_error_page() -> Html<String> {
    match fs::read_to_string("static/error.html") {
        Ok(contents) => Html(contents),
        Err(_) => Html("<h1>Internal Server Error</h1>".to_string()),
    }
}

pub async fn render_html_with_media(file_path: &str, media_dir: &str, media_route: &str, sort_method: Option<&str>) -> Html<String> {
    let mut content = match fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            print_fancy(&[
                ("Error: Unable to read file: ", RED, vec![]),
                (file_path, RED, vec![BOLD]),
                (&format!(" - {}", &e), RED, vec![])
            ], NewLine);
            return render_error_page().await;
        }
    };

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

    if let Some(insertion_point) = content.find("<!-- MEDIA_INSERTION_POINT -->") {
        let newline_index = content[..insertion_point].rfind('\n').unwrap_or(0);
        let indentation = &content[newline_index+1..insertion_point];
        let media_tags = media_files.iter().map(|file| {
            let indent = indentation;
            let linebreak = "\n";
            if is_video_file(file) {
                format!("{}<video controls><source src='/static/{}/{}' type='video/{}'></video>{}{}", indent, media_route, file, get_video_mime_type(file), file, linebreak)
            } else if is_audio_file(file) {
                format!("{}<audio controls><source src='/static/{}/{}' type='audio/{}'></audio>{}", indent, media_route, file, get_audio_mime_type(file), linebreak)
            } else if is_pdf_file(file) {
                format!("{}<iframe src='/static/{}/{}' width='100%' height='600px'></iframe>{}", &indent, media_route, file, linebreak)
            } else if is_image_file(file) {
                format!("{}<img src='/static/{}/{}'>{}", &indent, media_route, file, linebreak)
            } else if is_zip_file(file) {
                format!("{}<a href=\"/static/{}/{}\" download>{}</a>{}", &indent, media_route, file, file, linebreak)
            } else if is_markdown_file(file) {
                let post_name = file.trim_end_matches(".md");
                format!("{}<a href='/blog/{}'><h2>{}</h2></a>{}", &indent, post_name, post_name, linebreak)
            } else {
                "".to_string()
            }
        }).collect::<String>();
        content = content.replacen("<!-- MEDIA_INSERTION_POINT -->", &media_tags, 1);
    }

    if let Some(insertion_point) = content.find("<!-- THUMBNAIL_INSERTION_POINT -->") {
        let newline_index = content[..insertion_point].rfind('\n').unwrap_or(0);
        let indentation = &content[newline_index+1..insertion_point];
        let thumbnail_tags = media_files.iter().filter(|file| is_image_file(file)).map(|file| {
            format!("{}<a href='/static/{}/{}'><img src='/thumbnail/{}/{}'></a>\n", &indentation, media_route, file, media_dir, file)
        }).collect::<String>();
        content = content.replacen("<!-- THUMBNAIL_INSERTION_POINT -->", &thumbnail_tags, 1);
    }

    if let Some(_js_insertion_point) = content.find("<!-- JS_INSERTION_POINT -->") {
        let audio_files = media_files.iter()
            .filter(|file| is_audio_file(file))
            .map(|file| format!("'static/audio/{}'", &file))
            .collect::<Vec<_>>().join(", ");
        let js_playlist = format!("const playlist = [{}];", &audio_files);
        content = content.replacen("<!-- JS_INSERTION_POINT -->", &js_playlist, 1);
    }

    Html(content)
}


pub async fn render_html(file_path: &str) -> Html<String> {
    let content = match fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            print_fancy(&[
                ("Error: Unable to read file: ", RED, vec![]),
                (file_path, RED, vec![BOLD]),
                (&format!(" - {}", &e), RED, vec![])
            ], NewLine);
            return render_error_page().await;
        }
    };
    Html(content)
}
