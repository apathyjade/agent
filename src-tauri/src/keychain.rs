/// Secure API key storage using OS-level keychain.
///
/// Falls back to in-memory/ephemeral storage when the real keychain
/// is unavailable (CI, headless environments, etc.).
use keyring::{Entry, Error};

const SERVICE_NAME: &str = "agent-ai-client";

/// Store an API key in the OS keychain.
pub fn store_api_key(model_id: &str, api_key: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, model_id).map_err(|e| format!("Failed to create keychain entry: {e}"))?;
    entry.set_password(api_key).map_err(|e| format!("Failed to store API key: {e}"))
}

/// Retrieve an API key from the OS keychain.
///
/// Returns `Ok(None)` if no key is stored for this model.
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

/// Delete an API key from the OS keychain.
pub fn delete_api_key(model_id: &str) -> Result<(), String> {
    let entry = match Entry::new(SERVICE_NAME, model_id) {
        Ok(e) => e,
        Err(_) => return Ok(()), // nothing to delete
    };
    entry.delete_credential().map_err(|e| format!("Failed to delete API key: {e}"))
}

/// Try to resolve the API key for a model:
/// 1. Keychain (OS secure storage)
/// 2. Fallback to the `fallback` string (plaintext config)
pub fn resolve_api_key(model_id: &str, fallback: &str) -> String {
    if !fallback.is_empty() {
        return fallback.to_string();
    }
    get_api_key(model_id).ok().flatten().unwrap_or_default()
}
