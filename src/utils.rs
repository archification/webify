use tokio::fs;
use std::sync::Arc;
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

pub async fn build_tls_config(cert_path: &str, key_path: &str) -> anyhow::Result<Arc<ServerConfig>> {
    let cert_pem = tokio::fs::read(cert_path).await?;
    let key_pem = tokio::fs::read(key_path).await?;

    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_pem.as_slice())
        .collect::<Result<Vec<_>, _>>()?;

    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut key_pem.as_slice())?
        .ok_or_else(|| anyhow::anyhow!("no private key found in {key_path}"))?;

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(Arc::new(config))
}

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
