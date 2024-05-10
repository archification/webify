use axum::{
    response::Html,
    extract::Multipart,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::{path::Path};
use bytes::Bytes;
use tokio::{fs::{self, File}, io::AsyncWriteExt};
use sanitize_filename;
use serde::Serialize;

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

pub async fn upload(mut multipart: Multipart) -> impl IntoResponse {
    while let Some(field) = multipart.next_field().await.transpose() {
        match field {
            Ok(mut field) => {
                if let Some(filename) = field.file_name() {
                    let sanitized_filename = sanitize_filename::sanitize(filename);
                    let filepath = Path::new("./uploads").join(sanitized_filename);
                    if let Some(parent) = filepath.parent() {
                        if let Err(e) = fs::create_dir_all(parent).await {
                            eprintln!("Failed to create directory: {:?}", e);
                            return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Failed to create directory".to_string() })));
                        }
                    }
                    let mut file = match File::create(&filepath).await {
                        Ok(file) => file,
                        Err(e) => {
                            eprintln!("Failed to create file: {:?}", e);
                            return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Failed to create file".to_string() })));
                        },
                    };
                    while let Some(chunk) = field.chunk().await.transpose() {
                        match chunk {
                            Ok(data) => {
                                let data = Bytes::from(data);
                                if let Err(e) = file.write_all(&data).await {
                                    eprintln!("Failed to write to file: {:?}", e);
                                    return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Failed to write to file".to_string() })));
                                }
                            },
                            Err(e) => {
                                eprintln!("Failed to read chunk: {:?}", e);
                                return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Failed to read chunk".to_string() })));
                            }
                        }
                    }
                }
            },
            Err(e) => {
                eprintln!("Failed to retrieve field: {:?}", e);
                return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Failed to retrieve field".to_string() })));
            }
        }
    }
    Ok(Html("File uploaded successfully".to_string()))
}
