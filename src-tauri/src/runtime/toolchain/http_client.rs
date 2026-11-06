//! 运行时下载用的 HTTP 客户端。
//!
//! 提供一个全局单例 HTTP 客户端，会遵循代理设置。应用启动时用配置的代理
//! 地址初始化一次。

use std::sync::OnceLock;

use futures::StreamExt;

use crate::core::error::Result;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// 用可选的代理设置初始化全局 HTTP 客户端。
/// 应在应用启动时调用一次；重复调用不会覆盖已初始化的客户端。
pub fn init_http_client(proxy_url: Option<&str>) {
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(300));
    if let Some(proxy) = proxy_url {
        if !proxy.is_empty() {
            if let Ok(p) = reqwest::Proxy::all(proxy) {
                builder = builder.proxy(p);
            }
        }
    }
    match builder.build() {
        Ok(client) => {
            let _ = HTTP_CLIENT.set(client);
        }
        Err(error) => log::warn!("Failed to build HTTP client: {}", error),
    }
}

/// 获取全局 HTTP 客户端；未初始化时用默认配置懒加载。
pub fn get_http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                log::warn!("Failed to build HTTP client, using defaults: {}", error);
                reqwest::Client::new()
            }
        }
    })
}

/// 从 URL 下载字节，并上报下载进度。
///
/// `on_progress` 接收 `(已下载字节, 总字节)`；服务器未提供 `Content-Length`
/// 时不会触发回调。这是网络操作，会阻塞直到下载完成。
pub async fn download_bytes<F: Fn(u64, u64) + Send + 'static>(
    url: &str,
    on_progress: F,
) -> Result<Vec<u8>> {
    let client = get_http_client();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(crate::core::error::AppError::Http)?;

    if !response.status().is_success() {
        return Err(crate::core::error::AppError::Http(
            response.error_for_status().unwrap_err(),
        ));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(crate::core::error::AppError::Http)?;
        downloaded += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
        if total_size > 0 {
            on_progress(downloaded, total_size);
        }
    }

    Ok(bytes)
}

/// 从 URL 下载字节（不报告进度）。
pub async fn download_bytes_simple(url: &str) -> Result<Vec<u8>> {
    let client = get_http_client();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(crate::core::error::AppError::Http)?;

    if !response.status().is_success() {
        return Err(crate::core::error::AppError::Http(
            response.error_for_status().unwrap_err(),
        ));
    }

    Ok(response
        .bytes()
        .await
        .map_err(crate::core::error::AppError::Http)?
        .to_vec())
}
