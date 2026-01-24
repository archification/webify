use axum::{
    response::{Html, IntoResponse, Json, Response},
    extract::{Multipart},
    http::{HeaderMap, StatusCode},
};
use std::path::Path;
use bytes::Bytes;
use tokio::{
    fs::{self, File, metadata},
    io::AsyncWriteExt,
};
use futures::stream::{self, StreamExt};
use sanitize_filename;
use walkdir::WalkDir;
use serde_json::json;

enum UploadResponse {
    Html(Html<String>),
    Json(Json<serde_json::Value>),
    Error(StatusCode, String),
}

impl IntoResponse for UploadResponse {
    fn into_response(self) -> Response {
        match self {
            UploadResponse::Html(html) => html.into_response(),
            UploadResponse::Json(json) => json.into_response(),
            UploadResponse::Error(code, msg) => (code, msg).into_response(),
        }
    }
}

async fn get_directory_size<P: AsRef<Path>>(path: P) -> u64 {
    let entries = WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect::<Vec<_>>();
    let entry_stream = stream::iter(entries);
    let total_size = entry_stream
        .then(|entry| async {
            match metadata(entry).await {
                Ok(meta) => meta.len(),
                Err(_) => 0,
            }
        })
        .fold(0, |acc, size| async move { acc + size })
        .await;
    total_size
}

pub async fn upload(headers: HeaderMap, mut multipart: Multipart, upload_storage_limit: Option<u64>) -> impl IntoResponse {
    let wants_json = headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json"))
        .unwrap_or(false);

    let mut uploaded_files = Vec::new();

    while let Some(field) = multipart.next_field().await.transpose() {
        match field {
            Ok(mut field) => {
                if let Some(filename) = field.file_name() {
                    let sanitized_filename = sanitize_filename::sanitize(filename);
                    let filepath = Path::new("./uploads").join(&sanitized_filename);
                    
                    if let Some(parent) = filepath.parent() {
                        if let Err(e) = fs::create_dir_all(parent).await {
                            eprintln!("Failed to create directory: {:?}", e);
                            if wants_json {
                                return UploadResponse::Error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to create directory".into());
                            } else {
                                return UploadResponse::Html(Html(error_html("Failed to create directory")));
                            }
                        }
                    }

                    let mut file = match File::create(&filepath).await {
                        Ok(file) => file,
                        Err(e) => {
                            eprintln!("Failed to create file: {:?}", e);
                            if wants_json {
                                return UploadResponse::Error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to create file".into());
                            } else {
                                return UploadResponse::Html(Html(error_html("Failed to create file")));
                            }
                        },
                    };

                    while let Some(chunk) = field.chunk().await.transpose() {
                        match chunk {
                            Ok(data) => {
                                let data = Bytes::from(data);
                                let uploads_path = Path::new("./uploads");
                                let current_size = get_directory_size(uploads_path).await;
                                if let Some(limit) = upload_storage_limit {
                                    let new_size = current_size + data.len() as u64;
                                    if new_size > limit {
                                        if let Err(e) = fs::remove_file(&filepath).await {
                                            eprintln!("Failed to delete file: {:?}", e);
                                        }
                                        if wants_json {
                                            return UploadResponse::Error(StatusCode::PAYLOAD_TOO_LARGE, "Upload limit exceeded".into());
                                        } else {
                                            return UploadResponse::Html(Html(limit_exceeded_html(current_size)));
                                        }
                                    }
                                }
                                if let Err(e) = file.write_all(&data).await {
                                    eprintln!("Failed to write to file: {:?}", e);
                                    if wants_json {
                                        return UploadResponse::Error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to write to file".into());
                                    } else {
                                        return UploadResponse::Html(Html(error_html("Failed to write to file")));
                                    }
                                }
                            },
                            Err(e) => {
                                eprintln!("Failed to read chunk: {:?}", e);
                                if wants_json {
                                    return UploadResponse::Error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to read chunk".into());
                                } else {
                                    return UploadResponse::Html(Html(error_html("Failed to read chunk")));
                                }
                            }
                        }
                    }
                    uploaded_files.push(sanitized_filename);
                }
            },
            Err(e) => {
                eprintln!("Failed to retrieve field: {:?}", e);
                if wants_json {
                    return UploadResponse::Error(StatusCode::BAD_REQUEST, "Failed to retrieve field".into());
                } else {
                    return UploadResponse::Html(Html(error_html("Failed to retrieve field")));
                }
            }
        }
    }

    if wants_json {
        // Return the first file uploaded, or a list if needed. 
        // For chat, we usually upload one at a time.
        if let Some(filename) = uploaded_files.first() {
            UploadResponse::Json(Json(json!({
                "status": "success",
                "filename": filename,
                "url": format!("/uploads/{}", filename)
            })))
        } else {
            UploadResponse::Error(StatusCode::BAD_REQUEST, "No file uploaded".into())
        }
    } else {
        UploadResponse::Html(Html(success_html()))
    }
}

fn error_html(msg: &str) -> String {
    format!(r#"<!doctype html>
<html lang="en_US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>Error</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
</head>
<body>
    <h1>{}</h1>
</body>
</html>"#, msg)
}

fn limit_exceeded_html(size: u64) -> String {
    format!(r#"<!doctype html>
<html lang="en_US">
<head>
    <meta charset="utf-8" />
    <title>Upload Limit Exceeded</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
</head>
<body>
    <h1>Upload Limit Exceeded</h1>
    <p>Current storage use: {} bytes</p>
</body>
</html>"#, size)
}

fn success_html() -> String {
    r#"<!doctype html>
<html lang="en_US">
<head>
    <meta charset="utf-8" />
    <title>Success</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
</head>
<body>
    <h1>Upload Successful</h1>
    <br>
    <a href="/upload">Upload Another File</a>
    <br>
    <br>
    <a href="/files">Show Uploaded Files</a>
</body>
</html>"#.to_string()
}
