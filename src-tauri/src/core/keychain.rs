//! 基于操作系统钥匙串的 API Key 安全存储。
//!
//! 真实钥匙串不可用时（CI、无头环境等），读取会返回 `Ok(None)`、
//! 写入与删除会返回错误，由上层决定是否退回明文配置。

use keyring::{Entry, Error};

const SERVICE_NAME: &str = "agent-ai-client";

/// 把某个模型的 API Key 写入操作系统钥匙串，覆盖同名旧值。
pub fn store_api_key(model_id: &str, api_key: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, model_id).map_err(|e| format!("Failed to create keychain entry: {e}"))?;
    entry.set_password(api_key).map_err(|e| format!("Failed to store API key: {e}"))
}

/// 从操作系统钥匙串读取 API Key。
///
/// 未存储或钥匙串不可用时返回 `Ok(None)`，这两种情况都不视为错误。
pub fn get_api_key(model_id: &str) -> Result<Option<String>, String> {
    let entry = match Entry::new(SERVICE_NAME, model_id) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    match entry.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Failed to read API key: {e}")),
    }
}

/// 删除某个模型的 API Key；不存在或钥匙串不可用时静默成功（幂等）。
pub fn delete_api_key(model_id: &str) -> Result<(), String> {
    let entry = match Entry::new(SERVICE_NAME, model_id) {
        Ok(e) => e,
        Err(_) => return Ok(()), // 没有可删除的内容
    };
    entry.delete_credential().map_err(|e| format!("Failed to delete API key: {e}"))
}

/// 解析某个模型最终使用的 API Key。
///
/// 优先使用传入的明文 `fallback`；它为空时才查操作系统钥匙串。
/// 两者都取不到时返回空串，调用方需自行判断是否可用。
pub fn resolve_api_key(model_id: &str, fallback: &str) -> String {
    if !fallback.is_empty() {
        return fallback.to_string();
    }
    get_api_key(model_id).ok().flatten().unwrap_or_default()
}
