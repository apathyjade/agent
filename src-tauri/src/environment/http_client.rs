// ── HTTP Client for Runtime Downloads ──
//
// Provides a global singleton HTTP client that respects proxy settings.
// Initialized once at app startup with the configured proxy URL.

use std::sync::OnceLock;

use futures::StreamExt;

use crate::error::Result;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Initialize the global HTTP client with optional proxy settings.
/// Should be called once at application startup.
pub fn init_http_client(proxy_url: Option<&str>) {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300));
    if let Some(proxy) = proxy_url {
        if !proxy.is_empty() {
            if let Ok(p) = reqwest::Proxy::all(proxy) {
                builder = builder.proxy(p);
            }
        }
    }
    let _ = HTTP_CLIENT.set(builder.build().expect("Failed to build HTTP client"));
}

/// Get the global HTTP client, initializing with defaults if not set.
pub fn get_http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to build HTTP client")
    })
}

/// Download bytes from a URL with progress reporting.
pub async fn download_bytes<F: Fn(u64, u64) + Send + 'static>(
    url: &str,
    on_progress: F,
) -> Result<Vec<u8>> {
    let client = get_http_client();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| crate::error::AppError::Http(e))?;

    if !response.status().is_success() {
        return Err(crate::error::AppError::Http(
            response.error_for_status().unwrap_err(),
        ));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| crate::error::AppError::Http(e))?;
        downloaded += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
        if total_size > 0 {
            on_progress(downloaded, total_size);
        }
    }

    Ok(bytes)
}

/// Download bytes from a URL (no progress).
pub async fn download_bytes_simple(url: &str) -> Result<Vec<u8>> {
    let client = get_http_client();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| crate::error::AppError::Http(e))?;

    if !response.status().is_success() {
        return Err(crate::error::AppError::Http(
            response.error_for_status().unwrap_err(),
        ));
    }

    Ok(response
        .bytes()
        .await
        .map_err(|e| crate::error::AppError::Http(e))?
        .to_vec())
}
