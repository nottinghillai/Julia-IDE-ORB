use anyhow::Result;
use credentials_provider::CredentialsProvider;
use gpui::AsyncApp;
use std::sync::Arc;
use zed_env_vars::EnvVar;

/// Loads an API key for a web search provider from environment variables or the system keychain.
///
/// Checks the environment variable first, then falls back to the system keychain.
/// Returns `None` if no key is found.
pub async fn load_api_key(
    provider_name: &str,
    env_var_name: &str,
    cx: &AsyncApp,
) -> Result<Option<Arc<str>>> {
    // Check environment variable first
    let env_var_name = env_var_name.to_string();
    let env_var = EnvVar::new(env_var_name.into());
    if let Some(key) = env_var.value.as_ref() {
        if !key.is_empty() {
            return Ok(Some(key.as_str().into()));
        }
    }

    // Fall back to system keychain
    // Get the App from AsyncApp to access CredentialsProvider::global()
    let credentials_provider = cx.update(|cx| <dyn CredentialsProvider>::global(cx))?;
    let keychain_url = format!("web_search_provider://{}", provider_name);
    let credentials = credentials_provider.read_credentials(&keychain_url, cx).await?;

    if let Some((_, key_bytes)) = credentials {
        let key_str = std::str::from_utf8(&key_bytes)?;
        if !key_str.is_empty() {
            return Ok(Some(key_str.into()));
        }
    }

    Ok(None)
}

/// Checks if an API key is available for a provider (from env var or keychain).
#[allow(dead_code)]
pub fn has_api_key_sync(env_var_name: &str) -> bool {
    let env_var_name = env_var_name.to_string();
    let env_var = EnvVar::new(env_var_name.into());
    env_var.value.as_ref().is_some_and(|v| !v.is_empty())
}

/// Stores an API key in the system keychain for a web search provider.
#[allow(dead_code)]
pub async fn store_api_key(
    provider_name: &str,
    key: Option<String>,
    cx: &AsyncApp,
) -> Result<()> {
    // Get the App from AsyncApp to access CredentialsProvider::global()
    let credentials_provider = cx.update(|cx| <dyn CredentialsProvider>::global(cx))?;
    let keychain_url = format!("web_search_provider://{}", provider_name);

    if let Some(key) = &key {
        credentials_provider
            .write_credentials(&keychain_url, "Bearer", key.as_bytes(), cx)
            .await?;
    } else {
        credentials_provider
            .delete_credentials(&keychain_url, cx)
            .await?;
    }

    Ok(())
}
