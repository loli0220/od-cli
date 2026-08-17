use crate::auth::AuthManager;
use crate::config::Config;
use crate::types::*;
use crate::ui::{create_download_progress, create_upload_progress};
use anyhow::{bail, Context, Result};
use bytes::Bytes;
use colored::Colorize;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE};
use reqwest::{Client, StatusCode};
use std::fs::File as StdFile;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

const GRAPH_BASE_URL: &str = "https://graph.microsoft.com/v1.0";
const SIMPLE_UPLOAD_MAX_BYTES: u64 = 4 * 1024 * 1024; // 4MB

pub struct OneDriveClient {
    http: Client,
    auth_manager: Arc<AuthManager>,
    config: Arc<Mutex<Config>>,
}

impl OneDriveClient {
    pub fn new(auth_manager: Arc<AuthManager>, config: Arc<Mutex<Config>>) -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            auth_manager,
            config,
        }
    }

    pub async fn get_token(&self) -> Result<String> {
        let mut conf = self.config.lock().await;
        self.auth_manager.ensure_valid_token(&mut conf).await
    }

    pub fn normalize_path(path: &str) -> String {
        let cleaned = path.replace('\\', "/");
        let parts: Vec<&str> = cleaned
            .split('/')
            .filter(|p| !p.is_empty() && *p != ".")
            .collect();

        let mut stack = Vec::new();
        for part in parts {
            if part == ".." {
                stack.pop();
            } else {
                stack.push(part);
            }
        }

        if stack.is_empty() {
            String::new()
        } else {
            stack.join("/")
        }
    }

    fn item_endpoint(normalized_path: &str) -> String {
        if normalized_path.is_empty() {
            format!("{}/me/drive/root", GRAPH_BASE_URL)
        } else {
            format!("{}/me/drive/root:/{}:", GRAPH_BASE_URL, normalized_path)
        }
    }

    fn children_endpoint(normalized_path: &str) -> String {
        if normalized_path.is_empty() {
            format!("{}/me/drive/root/children", GRAPH_BASE_URL)
        } else {
            format!("{}/me/drive/root:/{}:/children", GRAPH_BASE_URL, normalized_path)
        }
    }

    fn content_endpoint(normalized_path: &str) -> String {
        if normalized_path.is_empty() {
            format!("{}/me/drive/root/content", GRAPH_BASE_URL)
        } else {
            format!("{}/me/drive/root:/{}:/content", GRAPH_BASE_URL, normalized_path)
        }
    }

    fn upload_session_endpoint(normalized_path: &str) -> String {
        if normalized_path.is_empty() {
            format!("{}/me/drive/root/createUploadSession", GRAPH_BASE_URL)
        } else {
            format!("{}/me/drive/root:/{}:/createUploadSession", GRAPH_BASE_URL, normalized_path)
        }
    }

    pub async fn get_drive(&self) -> Result<Drive> {
        let token = self.get_token().await?;
        let url = format!("{}/me/drive", GRAPH_BASE_URL);

        let res = self.http.get(&url).bearer_auth(token).send().await?;

        if res.status().is_success() {
            let drive: Drive = res.json().await?;
            Ok(drive)
        } else {
            let err_text = res.text().await.unwrap_or_default();
            bail!("Failed to get drive information: {}", err_text);
        }
    }

    pub async fn get_item(&self, remote_path: &str) -> Result<DriveItem> {
        let token = self.get_token().await?;
        let norm = Self::normalize_path(remote_path);
        let url = Self::item_endpoint(&norm);

        let res = self.http.get(&url).bearer_auth(token).send().await?;

        if res.status().is_success() {
            let item: DriveItem = res.json().await?;
            Ok(item)
        } else if res.status() == StatusCode::NOT_FOUND {
            bail!("Item not found at path '{}'", remote_path);
        } else {
            let err_text = res.text().await.unwrap_or_default();
            bail!("Failed to get item '{}': {}", remote_path, err_text);
        }
    }

    pub async fn list_children(&self, remote_path: &str) -> Result<Vec<DriveItem>> {
        let token = self.get_token().await?;
        let norm = Self::normalize_path(remote_path);
        let mut url = Self::children_endpoint(&norm);

        let mut all_items = Vec::new();

        loop {
            let res = self.http.get(&url).bearer_auth(&token).send().await?;

            if !res.status().is_success() {
                if res.status() == StatusCode::NOT_FOUND {
                    bail!("Directory not found at path '{}'", remote_path);
                }
                let err_text = res.text().await.unwrap_or_default();
                bail!("Failed to list directory contents: {}", err_text);
            }

            let list: DriveItemList = res.json().await?;
            all_items.extend(list.value);

            if let Some(next) = list.next_link {
                url = next;
            } else {
                break;
            }
        }

        // Sort folders first, then alphabetically by name
        all_items.sort_by(|a, b| match (a.is_dir(), b.is_dir()) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Ok(all_items)
    }

    pub async fn create_folder(&self, remote_path: &str, recursive: bool) -> Result<DriveItem> {
        let norm = Self::normalize_path(remote_path);
        if norm.is_empty() {
            bail!("Cannot create root folder");
        }

        let segments: Vec<&str> = norm.split('/').collect();

        if recursive && segments.len() > 1 {
            let mut current_path = String::new();
            for seg in segments {
                let next_path = if current_path.is_empty() {
                    seg.to_string()
                } else {
                    format!("{}/{}", current_path, seg)
                };

                // Check if exists
                if self.get_item(&next_path).await.is_err() {
                    let parent_norm = Self::normalize_path(&current_path);
                    let url = Self::children_endpoint(&parent_norm);
                    let body = CreateFolderRequest {
                        name: seg.to_string(),
                        folder: serde_json::json!({}),
                        conflict_behavior: Some("replace".to_string()),
                    };

                    let token = self.get_token().await?;
                    let res = self
                        .http
                        .post(&url)
                        .bearer_auth(token)
                        .json(&body)
                        .send()
                        .await?;

                    if !res.status().is_success() && res.status() != StatusCode::CONFLICT {
                        let err_text = res.text().await.unwrap_or_default();
                        bail!("Failed to create folder '{}': {}", next_path, err_text);
                    }
                }
                current_path = next_path;
            }

            self.get_item(&norm).await
        } else {
            let (parent_norm, folder_name) = match norm.rfind('/') {
                Some(idx) => (norm[..idx].to_string(), &norm[idx + 1..]),
                None => (String::new(), norm.as_str()),
            };

            let url = Self::children_endpoint(&parent_norm);
            let body = CreateFolderRequest {
                name: folder_name.to_string(),
                folder: serde_json::json!({}),
                conflict_behavior: Some("rename".to_string()),
            };

            let token = self.get_token().await?;
            let res = self
                .http
                .post(&url)
                .bearer_auth(token)
                .json(&body)
                .send()
                .await?;

            if res.status().is_success() {
                let item: DriveItem = res.json().await?;
                Ok(item)
            } else {
                let err_text = res.text().await.unwrap_or_default();
                bail!("Failed to create folder '{}': {}", remote_path, err_text);
            }
        }
    }

    pub async fn delete_item(&self, remote_path: &str) -> Result<()> {
        let norm = Self::normalize_path(remote_path);
        if norm.is_empty() {
            bail!("Cannot delete the root folder");
        }

        let token = self.get_token().await?;
        let url = Self::item_endpoint(&norm);

        let res = self.http.delete(&url).bearer_auth(token).send().await?;

        if res.status().is_success() || res.status() == StatusCode::NO_CONTENT {
            Ok(())
        } else if res.status() == StatusCode::NOT_FOUND {
            bail!("Item not found at path '{}'", remote_path);
        } else {
            let err_text = res.text().await.unwrap_or_default();
            bail!("Failed to delete item '{}': {}", remote_path, err_text);
        }
    }

    pub async fn move_item(&self, source_path: &str, target_path: &str) -> Result<DriveItem> {
        let src_norm = Self::normalize_path(source_path);
        let tgt_norm = Self::normalize_path(target_path);

        if src_norm.is_empty() {
            bail!("Cannot move the root folder");
        }

        let src_item = self.get_item(&src_norm).await?;

        // Determine new parent and new name
        let (tgt_parent_path, new_name) = match tgt_norm.rfind('/') {
            Some(idx) => (&tgt_norm[..idx], &tgt_norm[idx + 1..]),
            None => ("", tgt_norm.as_str()),
        };

        let mut req_body = MoveOrRenameRequest {
            name: Some(new_name.to_string()),
            parent_reference: None,
        };

        if !tgt_parent_path.is_empty() {
            let tgt_parent_item = self.get_item(tgt_parent_path).await?;
            req_body.parent_reference = Some(ItemReference {
                id: Some(tgt_parent_item.id),
                path: None,
            });
        } else if tgt_norm.contains('/') {
            // Target is root folder
            let root_item = self.get_item("").await?;
            req_body.parent_reference = Some(ItemReference {
                id: Some(root_item.id),
                path: None,
            });
        }

        let token = self.get_token().await?;
        let url = format!("{}/me/drive/items/{}", GRAPH_BASE_URL, src_item.id);

        let res = self
            .http
            .patch(&url)
            .bearer_auth(token)
            .json(&req_body)
            .send()
            .await?;

        if res.status().is_success() {
            let item: DriveItem = res.json().await?;
            Ok(item)
        } else {
            let err_text = res.text().await.unwrap_or_default();
            bail!("Failed to move item: {}", err_text);
        }
    }

    pub async fn copy_item(&self, source_path: &str, target_path: &str) -> Result<()> {
        let src_norm = Self::normalize_path(source_path);
        let tgt_norm = Self::normalize_path(target_path);

        if src_norm.is_empty() {
            bail!("Cannot copy the root folder");
        }

        let src_item = self.get_item(&src_norm).await?;

        let (tgt_parent_path, new_name) = match tgt_norm.rfind('/') {
            Some(idx) => (&tgt_norm[..idx], &tgt_norm[idx + 1..]),
            None => ("", tgt_norm.as_str()),
        };

        let parent_item = self.get_item(tgt_parent_path).await?;

        let body = CopyItemRequest {
            name: Some(new_name.to_string()),
            parent_reference: Some(ItemReference {
                id: Some(parent_item.id),
                path: None,
            }),
        };

        let token = self.get_token().await?;
        let url = format!("{}/me/drive/items/{}/copy", GRAPH_BASE_URL, src_item.id);

        let res = self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if res.status().is_success() || res.status() == StatusCode::ACCEPTED {
            Ok(())
        } else {
            let err_text = res.text().await.unwrap_or_default();
            bail!("Failed to copy item: {}", err_text);
        }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<DriveItem>> {
        let token = self.get_token().await?;
        let encoded_query = urlencoding::encode(query);
        let url = format!("{}/me/drive/root/search(q='{}')", GRAPH_BASE_URL, encoded_query);

        let res = self.http.get(&url).bearer_auth(token).send().await?;

        if res.status().is_success() {
            let list: DriveItemList = res.json().await?;
            Ok(list.value)
        } else {
            let err_text = res.text().await.unwrap_or_default();
            bail!("Failed to search OneDrive: {}", err_text);
        }
    }

    pub async fn create_share_link(
        &self,
        remote_path: &str,
        link_type: &str,
        scope: Option<&str>,
    ) -> Result<Permission> {
        let norm = Self::normalize_path(remote_path);
        let item = self.get_item(&norm).await?;

        let body = CreateLinkRequest {
            link_type: link_type.to_string(),
            scope: scope.map(|s| s.to_string()),
        };

        let token = self.get_token().await?;
        let url = format!("{}/me/drive/items/{}/createLink", GRAPH_BASE_URL, item.id);

        let res = self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if res.status().is_success() {
            let perm: Permission = res.json().await?;
            Ok(perm)
        } else {
            let err_text = res.text().await.unwrap_or_default();
            bail!("Failed to create share link: {}", err_text);
        }
    }

    pub async fn upload_file(
        &self,
        local_path: &Path,
        remote_path: &str,
        show_progress: bool,
    ) -> Result<DriveItem> {
        if !local_path.exists() {
            bail!("Local file not found at {:?}", local_path);
        }

        let metadata = std::fs::metadata(local_path)
            .with_context(|| format!("Failed to read metadata for {:?}", local_path))?;

        if !metadata.is_file() {
            bail!("{:?} is not a file", local_path);
        }

        let file_size = metadata.len();
        let norm = Self::normalize_path(remote_path);
        let file_name = local_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        // Target path
        let final_remote_path = if norm.is_empty() {
            file_name.to_string()
        } else {
            // Check if remote_path is an existing directory
            if let Ok(item) = self.get_item(&norm).await {
                if item.is_dir() {
                    format!("{}/{}", norm, file_name)
                } else {
                    norm
                }
            } else {
                norm
            }
        };

        let chunk_size = {
            let conf = self.config.lock().await;
            conf.get_chunk_size_bytes()
        };

        if file_size <= SIMPLE_UPLOAD_MAX_BYTES {
            self.upload_simple(local_path, &final_remote_path, file_size, show_progress).await
        } else {
            self.upload_chunked(local_path, &final_remote_path, file_size, chunk_size, show_progress).await
        }
    }

    async fn upload_simple(
        &self,
        local_path: &Path,
        remote_path: &str,
        file_size: u64,
        show_progress: bool,
    ) -> Result<DriveItem> {
        let token = self.get_token().await?;
        let url = Self::content_endpoint(remote_path);

        let pb = if show_progress {
            Some(create_upload_progress(
                file_size,
                &format!("Uploading {}", remote_path),
            ))
        } else {
            None
        };

        let file_bytes = std::fs::read(local_path)
            .with_context(|| format!("Failed to read local file {:?}", local_path))?;

        let mime = mime_guess::from_path(local_path)
            .first_or_octet_stream()
            .to_string();

        let res = self
            .http
            .put(&url)
            .bearer_auth(token)
            .header(CONTENT_TYPE, mime)
            .body(file_bytes)
            .send()
            .await?;

        if let Some(pb) = pb {
            pb.finish_with_message("Upload complete");
        }

        if res.status().is_success() || res.status() == StatusCode::CREATED {
            let item: DriveItem = res.json().await?;
            Ok(item)
        } else {
            let err_text = res.text().await.unwrap_or_default();
            bail!("Failed to upload file '{}': {}", remote_path, err_text);
        }
    }

    async fn upload_chunked(
        &self,
        local_path: &Path,
        remote_path: &str,
        total_size: u64,
        chunk_size: usize,
        show_progress: bool,
    ) -> Result<DriveItem> {
        let token = self.get_token().await?;
        let session_url = Self::upload_session_endpoint(remote_path);

        let session_req = CreateUploadSessionRequest {
            item: CreateUploadSessionItem {
                conflict_behavior: Some("replace".to_string()),
                description: None,
                name: None,
            },
        };

        let session_res = self
            .http
            .post(&session_url)
            .bearer_auth(&token)
            .json(&session_req)
            .send()
            .await?;

        if !session_res.status().is_success() {
            let err_text = session_res.text().await.unwrap_or_default();
            bail!("Failed to create upload session: {}", err_text);
        }

        let session: UploadSession = session_res.json().await?;
        let upload_url = session.upload_url;

        let pb = if show_progress {
            Some(create_upload_progress(
                total_size,
                &format!("Chunked upload {}", remote_path),
            ))
        } else {
            None
        };

        let mut file = StdFile::open(local_path)
            .with_context(|| format!("Failed to open local file {:?}", local_path))?;

        let mut offset: u64 = 0;
        let mut buffer = vec![0u8; chunk_size];

        while offset < total_size {
            let current_chunk_size = std::cmp::min(chunk_size as u64, total_size - offset) as usize;
            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(&mut buffer[..current_chunk_size])?;

            let chunk_data = Bytes::copy_from_slice(&buffer[..current_chunk_size]);
            let start = offset;
            let end = offset + current_chunk_size as u64 - 1;
            let range_header = format!("bytes {}-{}/{}", start, end, total_size);

            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_LENGTH, HeaderValue::from(current_chunk_size));
            headers.insert(
                CONTENT_RANGE,
                HeaderValue::from_str(&range_header).unwrap(),
            );

            let res = self
                .http
                .put(&upload_url)
                .headers(headers)
                .body(chunk_data)
                .send()
                .await?;

            if !res.status().is_success()
                && res.status() != StatusCode::ACCEPTED
                && res.status() != StatusCode::CREATED
            {
                let err_text = res.text().await.unwrap_or_default();
                if let Some(ref pb) = pb {
                    pb.abandon_with_message("Upload failed");
                }
                bail!("Chunk upload failed at byte {}: {}", offset, err_text);
            }

            offset += current_chunk_size as u64;
            if let Some(ref pb) = pb {
                pb.set_position(offset);
            }

            // Check if final item returned
            if (res.status() == StatusCode::CREATED || res.status() == StatusCode::OK)
                && let Ok(item) = res.json::<DriveItem>().await
            {
                if let Some(pb) = pb {
                    pb.finish_with_message("Upload complete");
                }
                return Ok(item);
            }
        }

        if let Some(pb) = pb {
            pb.finish_with_message("Upload complete");
        }

        self.get_item(remote_path).await
    }

    pub async fn download_file(
        &self,
        remote_path: &str,
        local_path: &Path,
        show_progress: bool,
    ) -> Result<()> {
        let item = self.get_item(remote_path).await?;
        if item.is_dir() {
            bail!("'{}' is a directory. Use download directory mode instead.", remote_path);
        }

        let target_file_path = if local_path.is_dir() {
            local_path.join(&item.name)
        } else {
            local_path.to_path_buf()
        };

        if let Some(parent) = target_file_path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)?;
        }

        let token = self.get_token().await?;
        let norm = Self::normalize_path(remote_path);
        let url = Self::content_endpoint(&norm);

        let res = self.http.get(&url).bearer_auth(token).send().await?;

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            bail!("Failed to download file '{}': {}", remote_path, err_text);
        }

        let total_size = item.size.unwrap_or(0);
        let pb = if show_progress && total_size > 0 {
            Some(create_download_progress(
                total_size,
                &format!("Downloading {}", item.name),
            ))
        } else {
            None
        };

        let mut file = tokio::fs::File::create(&target_file_path)
            .await
            .with_context(|| format!("Failed to create local file {:?}", target_file_path))?;

        let mut stream = res.bytes_stream();
        let mut downloaded: u64 = 0;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.context("Error downloading chunk stream")?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            if let Some(ref pb) = pb {
                pb.set_position(downloaded);
            }
        }

        file.flush().await?;

        if let Some(pb) = pb {
            pb.finish_with_message("Download complete");
        }

        Ok(())
    }

    pub async fn cat_file(&self, remote_path: &str) -> Result<()> {
        let item = self.get_item(remote_path).await?;
        if item.is_dir() {
            bail!("'{}' is a directory, cannot cat.", remote_path);
        }

        let token = self.get_token().await?;
        let norm = Self::normalize_path(remote_path);
        let url = Self::content_endpoint(&norm);

        let res = self.http.get(&url).bearer_auth(token).send().await?;

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            bail!("Failed to read file content: {}", err_text);
        }

        let mut stdout = tokio::io::stdout();
        let mut stream = res.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            stdout.write_all(&chunk).await?;
        }
        stdout.flush().await?;

        Ok(())
    }

    pub async fn upload_directory(
        &self,
        local_dir: &Path,
        remote_dir: &str,
    ) -> Result<()> {
        if !local_dir.is_dir() {
            bail!("{:?} is not a local directory", local_dir);
        }

        let remote_base = Self::normalize_path(remote_dir);
        self.create_folder(&remote_base, true).await?;

        let mut stack = vec![(local_dir.to_path_buf(), remote_base)];

        while let Some((curr_local, curr_remote)) = stack.pop() {
            let entries = std::fs::read_dir(&curr_local)?;
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let next_remote = if curr_remote.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{}", curr_remote, name)
                };

                if path.is_dir() {
                    self.create_folder(&next_remote, true).await?;
                    stack.push((path, next_remote));
                } else if path.is_file() {
                    println!("=> Uploading: {} -> {}", path.display(), next_remote.cyan());
                    self.upload_file(&path, &next_remote, true).await?;
                }
            }
        }

        Ok(())
    }

    pub async fn download_directory(
        &self,
        remote_dir: &str,
        local_dir: &Path,
    ) -> Result<()> {
        let norm_remote = Self::normalize_path(remote_dir);
        let item = self.get_item(&norm_remote).await?;
        if !item.is_dir() {
            bail!("'{}' is not a directory", remote_dir);
        }

        if !local_dir.exists() {
            std::fs::create_dir_all(local_dir)?;
        }

        let mut stack = vec![(norm_remote, local_dir.to_path_buf())];

        while let Some((curr_remote, curr_local)) = stack.pop() {
            if !curr_local.exists() {
                std::fs::create_dir_all(&curr_local)?;
            }

            let children = self.list_children(&curr_remote).await?;
            for child in children {
                let child_remote = if curr_remote.is_empty() {
                    child.name.clone()
                } else {
                    format!("{}/{}", curr_remote, child.name)
                };
                let child_local = curr_local.join(&child.name);

                if child.is_dir() {
                    stack.push((child_remote, child_local));
                } else {
                    println!("=> Downloading: {} -> {}", child_remote.cyan(), child_local.display());
                    self.download_file(&child_remote, &child_local, true).await?;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(OneDriveClient::normalize_path(""), "");
        assert_eq!(OneDriveClient::normalize_path("/"), "");
        assert_eq!(OneDriveClient::normalize_path("///"), "");
        assert_eq!(OneDriveClient::normalize_path("/folder/subfolder"), "folder/subfolder");
        assert_eq!(OneDriveClient::normalize_path("folder/subfolder/file.txt"), "folder/subfolder/file.txt");
        assert_eq!(OneDriveClient::normalize_path("folder/sub/../file.txt"), "folder/file.txt");
        assert_eq!(OneDriveClient::normalize_path(r"folder\sub\file.txt"), "folder/sub/file.txt");
        assert_eq!(OneDriveClient::normalize_path("/a/b/../../c"), "c");
    }
}
