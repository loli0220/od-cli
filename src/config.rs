use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_CLIENT_ID: &str = "d3590ed6-52b3-4102-aeff-aad2292ab01c";
pub const DEFAULT_TENANT_ID: &str = "common";
pub const DEFAULT_CHUNK_SIZE_MB: usize = 10; // Must be multiple of 320 KiB for MS Graph
pub const DEFAULT_THREADS: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub client_id: Option<String>,
    pub tenant_id: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub user_principal_name: Option<String>,
    pub display_name: Option<String>,
    pub chunk_size_mb: Option<usize>,
    pub ip_preference: Option<String>,
    pub threads: Option<usize>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            client_id: Some(DEFAULT_CLIENT_ID.to_string()),
            tenant_id: Some(DEFAULT_TENANT_ID.to_string()),
            access_token: None,
            refresh_token: None,
            expires_at: None,
            user_principal_name: None,
            display_name: None,
            chunk_size_mb: Some(DEFAULT_CHUNK_SIZE_MB),
            ip_preference: Some("auto".to_string()),
            threads: Some(DEFAULT_THREADS),
        }
    }
}

impl Config {
    pub fn config_dir() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .or_else(dirs::home_dir)
            .context("Failed to determine home or config directory")?
            .join("od-cli");
        Ok(config_dir)
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.json"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file at {}", path.display()))?;
        let config: Config = serde_json::from_str(&content)
            .with_context(|| "Failed to parse config file JSON")?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir()?;
        if !dir.exists() {
            fs::create_dir_all(&dir)
                .with_context(|| format!("Failed to create config directory at {}", dir.display()))?;
        }

        let path = Self::config_path()?;
        let content = serde_json::to_string_pretty(self)
            .with_context(|| "Failed to serialize config to JSON")?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write config file to {}", path.display()))?;
        Ok(())
    }

    pub fn get_client_id(&self) -> String {
        self.client_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string())
    }

    pub fn get_tenant_id(&self) -> String {
        self.tenant_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_TENANT_ID.to_string())
    }

    pub fn is_token_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => {
                // Buffer by 60 seconds
                Utc::now().timestamp() + 60 >= exp
            }
            None => true,
        }
    }

    pub fn clear_tokens(&mut self) {
        self.access_token = None;
        self.refresh_token = None;
        self.expires_at = None;
        self.user_principal_name = None;
        self.display_name = None;
    }

    pub fn get_chunk_size_bytes(&self) -> usize {
        let mb = self.chunk_size_mb.unwrap_or(DEFAULT_CHUNK_SIZE_MB).max(1);
        // OneDrive chunk size must be a multiple of 320 KiB (327,680 bytes)
        let unit = 320 * 1024;
        let bytes = mb * 1024 * 1024;
        (bytes / unit) * unit
    }

    pub fn get_ip_preference(&self) -> Option<&str> {
        self.ip_preference.as_deref()
    }

    pub fn get_threads(&self) -> usize {
        self.threads.unwrap_or(DEFAULT_THREADS).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_size_is_multiple_of_320_kib() {
        let mut config = Config::default();
        config.chunk_size_mb = Some(10);
        let bytes = config.get_chunk_size_bytes();
        assert_eq!(bytes % (320 * 1024), 0);
        assert!(bytes > 0);

        config.chunk_size_mb = Some(5);
        let bytes5 = config.get_chunk_size_bytes();
        assert_eq!(bytes5 % (320 * 1024), 0);
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.get_client_id(), DEFAULT_CLIENT_ID);
        assert_eq!(config.get_tenant_id(), DEFAULT_TENANT_ID);
        assert!(config.is_token_expired());
    }
}
