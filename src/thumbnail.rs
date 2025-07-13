use axum::{
    body::Body,
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use image::{ImageReader, ImageFormat};
use std::io::Cursor;

// Define a standard size for thumbnails
const THUMBNAIL_WIDTH: u32 = 150;
const THUMBNAIL_HEIGHT: u32 = 150;

pub async fn generate_thumbnail(Path(path): Path<String>) -> impl IntoResponse {
    println!("thumbnail is generating");
    let image_path = format!("{}", &path);

    // Read the image file from the disk
    let image_bytes = match tokio::fs::read(&image_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            println!("ERROR: Could not find image at path: {}", &image_path);
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from(format!("Image not found at path: {}", &image_path)))
                .unwrap();
        }
    };

    // Use the image crate to decode the image from memory
    let img = match ImageReader::new(Cursor::new(image_bytes))
        .with_guessed_format()
        .expect("Cursor IO should not fail")
        .decode()
    {
        Ok(img) => img,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Failed to decode image"))
                .unwrap();
        }
    };

    // Create a thumbnail while preserving the aspect ratio
    let thumbnail = img.thumbnail(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT);

    // Encode the new thumbnail into a buffer as a PNG
    let mut buffer = Cursor::new(Vec::new());
    if thumbnail.write_to(&mut buffer, ImageFormat::Png).is_err() {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("Failed to encode thumbnail"))
            .unwrap();
    }

    // Return the raw image data in the response
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .body(Body::from(buffer.into_inner()))
        .unwrap()
}
