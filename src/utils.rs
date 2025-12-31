use tokio::fs;

pub async fn read_media_files(dir: &str) -> std::io::Result<Vec<String>> {
    let mut dir_reader = fs::read_dir(dir).await?;
    let mut files = Vec::new();
    while let Some(entry) = dir_reader.next_entry().await? {
        if entry.file_type().await?.is_file() && let Some(file_name) = entry.file_name().to_str() {
                files.push(file_name.to_string());
        }
    }
    Ok(files)
}
