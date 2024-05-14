use axum::{
    response::Html,
    extract::Multipart,
    response::IntoResponse,
    //Json,
};
use std::path::Path;
use bytes::Bytes;
use tokio::{
    fs::{
        self, File, metadata,
    },
    io::AsyncWriteExt,
};
use futures::stream::{self, StreamExt};
use sanitize_filename;
//use serde::Serialize;
use walkdir::WalkDir;

/*
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}
*/

enum UploadResponse {
    Html(Html<String>),
    //Json(Json<ErrorResponse>),
}

impl IntoResponse for UploadResponse {
    fn into_response(self) -> axum::response::Response {
        match self {
            UploadResponse::Html(html) => html.into_response(),
            //UploadResponse::Json(json) => json.into_response(),
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

pub async fn upload(mut multipart: Multipart, upload_storage_limit: Option<u64>) -> impl IntoResponse {
    while let Some(field) = multipart.next_field().await.transpose() {
        match field {
            Ok(mut field) => {
                if let Some(filename) = field.file_name() {
                    let sanitized_filename = sanitize_filename::sanitize(filename);
                    let filepath = Path::new("./uploads").join(sanitized_filename);
                    if let Some(parent) = filepath.parent() {
                        if let Err(e) = fs::create_dir_all(parent).await {
                            eprintln!("Failed to create directory: {:?}", e);
                            return UploadResponse::Html(Html(r#"<!doctype html>
<html lang="en_US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>Error</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>Failed to create directory</h1>
</body>
</html>
"#.to_string()));
                        }
                    }
                    let mut file = match File::create(&filepath).await {
                        Ok(file) => file,
                        Err(e) => {
                            eprintln!("Failed to create file: {:?}", e);
                            return UploadResponse::Html(Html(r#"<!doctype html>
<html lang="en_US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>Error</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>Failed to create file</h1>
</body>
</html>
"#.to_string()));
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
                                        println!("Error: Upload limit exceeded");
                                        return UploadResponse::Html(Html(r#"<!doctype html>
<html lang="en_US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>Upload Limit Exceeded</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>Upload Limit Exceeded</h1>
    <p>Current storage use: {} bytes</p>
</body>
</html>
"#.to_string()));
                                    }
                                }
                                if let Err(e) = file.write_all(&data).await {
                                    eprintln!("Failed to write to file: {:?}", e);
                                    return UploadResponse::Html(Html(r#"<!doctype html>
<html lang="en_US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>Error</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>Failed to write to file</h1>
</body>
</html>
"#.to_string()));
                                }
                            },
                            Err(e) => {
                                eprintln!("Failed to read chunk: {:?}", e);
                                return UploadResponse::Html(Html(r#"<!doctype html>
<html lang="en_US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>Error</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>Failed to read chunk</h1>
</body>
</html>
"#.to_string()));
                            }
                        }
                    }
                }
            },
            Err(e) => {
                eprintln!("Failed to retrieve field: {:?}", e);
                return UploadResponse::Html(Html(r#"<!doctype html>
<html lang="en_US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>Error</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>Failed to retrieve field</h1>
</body>
</html>
"#.to_string()));
            }
        }
    }
    UploadResponse::Html(Html(r#"<!doctype html>
<html lang="en_US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>success</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>Upload Successful</h1>
</body>
</html>
"#.to_string()))
}
