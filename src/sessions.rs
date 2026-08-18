use crate::config::Config;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredUploadSession {
    pub upload_url: String,
    pub remote_path: String,
    pub local_path: String,
    pub file_size: u64,
    pub file_modified_secs: u64,
    pub expiration: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionStore {
    // Key: "{local_path}::{remote_path}"
    pub sessions: HashMap<String, StoredUploadSession>,
}

impl SessionStore {
    fn session_file_path() -> Result<PathBuf> {
        let dir = Config::config_dir()?;
        Ok(dir.join("upload_sessions.json"))
    }

    pub fn load() -> Self {
        if let Ok(path) = Self::session_file_path()
            && path.exists()
            && let Ok(content) = fs::read_to_string(&path)
            && let Ok(store) = serde_json::from_str::<SessionStore>(&content)
        {
            return store;
        }
        Self::default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::session_file_path()?;
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize upload sessions")?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write upload sessions to {}", path.display()))?;
        Ok(())
    }

    fn make_key(local_path: &Path, remote_path: &str) -> String {
        let local_canonical = local_path.to_string_lossy().replace('\\', "/");
        let remote_norm = crate::client::OneDriveClient::normalize_path(remote_path);
        format!("{}::{}", local_canonical, remote_norm)
    }

    pub fn get(
        &self,
        local_path: &Path,
        remote_path: &str,
        file_size: u64,
        file_modified_secs: u64,
    ) -> Option<&StoredUploadSession> {
        let key = Self::make_key(local_path, remote_path);
        if let Some(session) = self.sessions.get(&key)
            && session.file_size == file_size
            && session.file_modified_secs == file_modified_secs
        {
            return Some(session);
        }
        None
    }

    pub fn set(
        &mut self,
        local_path: &Path,
        remote_path: &str,
        file_size: u64,
        file_modified_secs: u64,
        upload_url: String,
        expiration: Option<String>,
    ) {
        let key = Self::make_key(local_path, remote_path);
        self.sessions.insert(
            key,
            StoredUploadSession {
                upload_url,
                remote_path: remote_path.to_string(),
                local_path: local_path.to_string_lossy().to_string(),
                file_size,
                file_modified_secs,
                expiration,
            },
        );
        let _ = self.save();
    }

    pub fn remove(&mut self, local_path: &Path, remote_path: &str) {
        let key = Self::make_key(local_path, remote_path);
        if self.sessions.remove(&key).is_some() {
            let _ = self.save();
        }
    }
}
