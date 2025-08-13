use axum::{
    body::Body,
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use image::{ImageReader, ImageFormat};
use std::io::Cursor;
use tokio::task;

const THUMBNAIL_WIDTH: u32 = 150;
const THUMBNAIL_HEIGHT: u32 = 150;

pub async fn generate_thumbnail(Path(path): Path<String>) -> impl IntoResponse {
    let image_bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            println!("ERROR: Could not find image at path: {}", &path);
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from(format!("Image not found at path: {}", &path)))
                .unwrap();
        }
    };
    let thumbnail_result = task::spawn_blocking(move || {
        let img = ImageReader::new(Cursor::new(image_bytes))
            .with_guessed_format()
            .expect("Cursor IO should not fail")
            .decode()?;
        let thumbnail = img.thumbnail(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT);
        let mut buffer = Cursor::new(Vec::new());
        thumbnail.write_to(&mut buffer, ImageFormat::Png)?;
        Ok::<_, image::ImageError>(buffer.into_inner())
    })
    .await;
    match thumbnail_result {
        Ok(Ok(buffer)) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/png")
            .body(Body::from(buffer))
            .unwrap(),
        _ => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("Failed to generate thumbnail"))
            .unwrap(),
    }
}
