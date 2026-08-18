use crate::auth::AuthManager;
use crate::config::Config;
use crate::sessions::SessionStore;
use crate::tasks::{TaskStore, TaskType, TransferTask};
use crate::types::*;
use crate::ui::{create_download_progress, create_upload_progress};
use anyhow::{Context, Result, bail};
use bytes::Bytes;
use colored::Colorize;
use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, HeaderMap, HeaderValue, RANGE};
use reqwest::{Client, StatusCode};
use std::fs::File as StdFile;
use std::io::{Read, Seek, SeekFrom};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Semaphore};

const GRAPH_BASE_URL: &str = "https://graph.microsoft.com/v1.0";
const SIMPLE_UPLOAD_MAX_BYTES: u64 = 4 * 1024 * 1024; // 4MB

pub struct OneDriveClient {
    http: Client,
    auth_manager: Arc<AuthManager>,
    config: Arc<Mutex<Config>>,
}

impl OneDriveClient {
    pub fn new(
        auth_manager: Arc<AuthManager>,
        config: Arc<Mutex<Config>>,
        ip_preference: Option<&str>,
    ) -> Self {
        let mut builder = Client::builder().timeout(Duration::from_secs(120));
        if let Some(pref) = ip_preference {
            match pref.to_lowercase().as_str() {
                "ipv4" | "v4" | "4" => {
                    builder = builder.local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
                }
                "ipv6" | "v6" | "6" => {
                    builder = builder.local_address(IpAddr::V6(Ipv6Addr::UNSPECIFIED));
                }
                _ => {}
            }
        }

        Self {
            http: builder.build().unwrap_or_default(),
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
            format!(
                "{}/me/drive/root:/{}:/children",
                GRAPH_BASE_URL, normalized_path
            )
        }
    }

    fn content_endpoint(normalized_path: &str) -> String {
        if normalized_path.is_empty() {
            format!("{}/me/drive/root/content", GRAPH_BASE_URL)
        } else {
            format!(
                "{}/me/drive/root:/{}:/content",
                GRAPH_BASE_URL, normalized_path
            )
        }
    }

    fn upload_session_endpoint(normalized_path: &str) -> String {
        if normalized_path.is_empty() {
            format!("{}/me/drive/root/createUploadSession", GRAPH_BASE_URL)
        } else {
            format!(
                "{}/me/drive/root:/{}:/createUploadSession",
                GRAPH_BASE_URL, normalized_path
            )
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
        let url = format!(
            "{}/me/drive/root/search(q='{}')",
            GRAPH_BASE_URL, encoded_query
        );

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
        threads: usize,
    ) -> Result<DriveItem> {
        if !local_path.exists() {
            bail!("Local file not found at {}", local_path.display());
        }

        let metadata = std::fs::metadata(local_path)
            .with_context(|| format!("Failed to read metadata for {}", local_path.display()))?;

        if !metadata.is_file() {
            bail!("{} is not a file", local_path.display());
        }

        let file_size = metadata.len();
        let modified_secs = metadata
            .modified()
            .ok()
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let norm = Self::normalize_path(remote_path);
        let file_name = local_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let final_remote_path = if norm.is_empty() {
            file_name.to_string()
        } else {
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

        let mut task_store = TaskStore::load();
        let task_id = task_store.create_or_get_task(
            TaskType::Upload,
            local_path,
            &final_remote_path,
            file_size,
            false,
            Some(threads),
        );

        let res = if file_size <= SIMPLE_UPLOAD_MAX_BYTES {
            self.upload_simple(local_path, &final_remote_path, file_size, show_progress)
                .await
        } else {
            self.upload_chunked(
                local_path,
                &final_remote_path,
                file_size,
                modified_secs,
                chunk_size,
                threads,
                show_progress,
            )
            .await
        };

        let mut task_store = TaskStore::load();
        match &res {
            Ok(_) => task_store.mark_completed(&task_id),
            Err(e) => task_store.mark_interrupted(&task_id, Some(e.to_string()), 0),
        }

        res
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
            .with_context(|| format!("Failed to read local file {}", local_path.display()))?;

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

    fn parse_expected_ranges(ranges: &[String]) -> Vec<(u64, u64)> {
        let mut parsed = Vec::new();
        for r in ranges {
            let parts: Vec<&str> = r.split('-').collect();
            if parts.len() == 2 {
                let start: u64 = parts[0].parse().unwrap_or(0);
                let end: u64 = if parts[1].is_empty() {
                    u64::MAX
                } else {
                    parts[1].parse().unwrap_or(u64::MAX)
                };
                parsed.push((start, end));
            }
        }
        parsed
    }

    fn is_chunk_needed(start: u64, end: u64, missing_ranges: &[(u64, u64)]) -> bool {
        if missing_ranges.is_empty() {
            return true;
        }
        missing_ranges
            .iter()
            .any(|&(r_start, r_end)| start <= r_end && end >= r_start)
    }

    #[allow(clippy::too_many_arguments)]
    async fn upload_chunked(
        &self,
        local_path: &Path,
        remote_path: &str,
        total_size: u64,
        modified_secs: u64,
        chunk_size: usize,
        threads: usize,
        show_progress: bool,
    ) -> Result<DriveItem> {
        let mut session_store = SessionStore::load();
        let mut upload_url: Option<String> = None;
        let mut missing_ranges: Vec<(u64, u64)> = Vec::new();

        // 1. Check for existing resumable session
        if let Some(stored) = session_store.get(local_path, remote_path, total_size, modified_secs)
            && let Ok(res) = self.http.get(&stored.upload_url).send().await
            && res.status().is_success()
            && let Ok(session) = res.json::<UploadSession>().await
            && let Some(ranges) = session.next_expected_ranges
        {
            missing_ranges = Self::parse_expected_ranges(&ranges);
            upload_url = Some(stored.upload_url.clone());
            if show_progress {
                println!("{}", "⚡ Found existing upload session, resuming...".cyan());
            }
        }

        // 2. If no valid session, create a new one
        let upload_url = match upload_url {
            Some(u) => u,
            None => {
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
                session_store.set(
                    local_path,
                    remote_path,
                    total_size,
                    modified_secs,
                    session.upload_url.clone(),
                    session.expiration_date_time,
                );
                session.upload_url
            }
        };

        // 3. Compute all chunk intervals
        let mut all_chunks: Vec<(u64, u64)> = Vec::new();
        let mut offset: u64 = 0;
        while offset < total_size {
            let current_chunk_size = std::cmp::min(chunk_size as u64, total_size - offset);
            let start = offset;
            let end = offset + current_chunk_size - 1;
            all_chunks.push((start, end));
            offset += current_chunk_size;
        }

        // Filter chunks that are needed
        let chunks_to_upload: Vec<(u64, u64)> = all_chunks
            .into_iter()
            .filter(|&(start, end)| Self::is_chunk_needed(start, end, &missing_ranges))
            .collect();

        let needed_bytes: u64 = chunks_to_upload
            .iter()
            .map(|&(start, end)| end - start + 1)
            .sum();
        let already_uploaded = total_size.saturating_sub(needed_bytes);

        let pb = if show_progress {
            let bar = create_upload_progress(total_size, &format!("Uploading {}", remote_path));
            bar.set_position(already_uploaded);
            Some(Arc::new(bar))
        } else {
            None
        };

        let concurrency = threads.max(1);
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let final_item: Arc<Mutex<Option<DriveItem>>> = Arc::new(Mutex::new(None));
        let local_path_buf = local_path.to_path_buf();
        let upload_url_arc = Arc::new(upload_url);

        let mut tasks = Vec::new();

        for (start, end) in chunks_to_upload {
            let sem = semaphore.clone();
            let http = self.http.clone();
            let url = upload_url_arc.clone();
            let path = local_path_buf.clone();
            let pb_clone = pb.clone();
            let item_ref = final_item.clone();

            tasks.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let current_chunk_size = (end - start + 1) as usize;

                // Read chunk bytes from file
                let chunk_data = {
                    let mut file = StdFile::open(&path)
                        .with_context(|| format!("Failed to open local file {}", path.display()))?;
                    file.seek(SeekFrom::Start(start))?;
                    let mut buf = vec![0u8; current_chunk_size];
                    file.read_exact(&mut buf)?;
                    Bytes::from(buf)
                };

                let range_header = format!("bytes {}-{}/{}", start, end, total_size);
                let mut headers = HeaderMap::new();
                headers.insert(CONTENT_LENGTH, HeaderValue::from(current_chunk_size));
                headers.insert(CONTENT_RANGE, HeaderValue::from_str(&range_header).unwrap());

                let res = http
                    .put(url.as_str())
                    .headers(headers)
                    .body(chunk_data)
                    .send()
                    .await?;

                let status = res.status();
                if !status.is_success()
                    && status != StatusCode::ACCEPTED
                    && status != StatusCode::CREATED
                {
                    let err_text = res.text().await.unwrap_or_default();
                    bail!("Chunk upload failed at byte {}: {}", start, err_text);
                }

                if let Some(ref p) = pb_clone {
                    p.inc(current_chunk_size as u64);
                }

                if (status == StatusCode::CREATED || status == StatusCode::OK)
                    && let Ok(item) = res.json::<DriveItem>().await
                {
                    let mut lock = item_ref.lock().await;
                    *lock = Some(item);
                }

                Ok::<(), anyhow::Error>(())
            }));
        }

        for task in tasks {
            task.await??;
        }

        if let Some(ref p) = pb {
            p.finish_with_message("Upload complete");
        }

        // Cleanup session upon success
        let mut session_store = SessionStore::load();
        session_store.remove(local_path, remote_path);

        let maybe_item = {
            let lock = final_item.lock().await;
            lock.clone()
        };

        if let Some(item) = maybe_item {
            Ok(item)
        } else {
            self.get_item(remote_path).await
        }
    }

    pub async fn download_file(
        &self,
        remote_path: &str,
        local_path: &Path,
        show_progress: bool,
    ) -> Result<()> {
        let item = self.get_item(remote_path).await?;
        if item.is_dir() {
            bail!(
                "'{}' is a directory. Use download directory mode instead.",
                remote_path
            );
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

        let total_size = item.size.unwrap_or(0);
        let mut task_store = TaskStore::load();
        let task_id = task_store.create_or_get_task(
            TaskType::Download,
            &target_file_path,
            remote_path,
            total_size,
            false,
            None,
        );

        // Part file for resumable download
        let part_file_path = PathBuf::from(format!("{}.part", target_file_path.display()));

        let mut existing_bytes = 0u64;
        if part_file_path.exists()
            && let Ok(meta) = std::fs::metadata(&part_file_path)
        {
            let len = meta.len();
            if len < total_size {
                existing_bytes = len;
                if show_progress && existing_bytes > 0 {
                    println!(
                        "{}",
                        format!("⚡ Resuming download from byte {}...", existing_bytes).cyan()
                    );
                }
            } else if len == total_size {
                // Already downloaded, rename and return
                tokio::fs::rename(&part_file_path, &target_file_path).await?;
                let mut task_store = TaskStore::load();
                task_store.mark_completed(&task_id);
                return Ok(());
            }
        }

        let token = self.get_token().await?;
        let norm = Self::normalize_path(remote_path);
        let url = Self::content_endpoint(&norm);

        let mut req = self.http.get(&url).bearer_auth(token);
        if existing_bytes > 0 && total_size > 0 {
            let range_val = format!("bytes={}-{}", existing_bytes, total_size - 1);
            req = req.header(RANGE, range_val);
        }

        let res = req.send().await;
        let res = match res {
            Ok(r) => r,
            Err(e) => {
                let mut task_store = TaskStore::load();
                task_store.mark_interrupted(&task_id, Some(e.to_string()), existing_bytes);
                return Err(e.into());
            }
        };

        if !res.status().is_success() && res.status() != StatusCode::PARTIAL_CONTENT {
            let err_text = res.text().await.unwrap_or_default();
            let mut task_store = TaskStore::load();
            task_store.mark_interrupted(&task_id, Some(err_text.clone()), existing_bytes);
            bail!("Failed to download file '{}': {}", remote_path, err_text);
        }

        let pb = if show_progress && total_size > 0 {
            let bar = create_download_progress(total_size, &format!("Downloading {}", item.name));
            bar.set_position(existing_bytes);
            Some(bar)
        } else {
            None
        };

        let mut file = if existing_bytes > 0 {
            tokio::fs::OpenOptions::new()
                .write(true)
                .append(true)
                .open(&part_file_path)
                .await?
        } else {
            tokio::fs::File::create(&part_file_path).await?
        };

        let mut stream = res.bytes_stream();
        let mut downloaded = existing_bytes;

        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    let mut task_store = TaskStore::load();
                    task_store.mark_interrupted(&task_id, Some(e.to_string()), downloaded);
                    return Err(e.into());
                }
            };
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

        // Rename .part file to final destination
        tokio::fs::rename(&part_file_path, &target_file_path).await?;

        let mut task_store = TaskStore::load();
        task_store.mark_completed(&task_id);

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
        threads: usize,
    ) -> Result<()> {
        if !local_dir.is_dir() {
            bail!("'{}' is not a local directory", local_dir.display());
        }

        let remote_base = Self::normalize_path(remote_dir);
        self.create_folder(&remote_base, true).await?;

        let mut dirs_to_create = Vec::new();
        let mut files_to_upload: Vec<(PathBuf, String)> = Vec::new();

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
                    dirs_to_create.push(next_remote.clone());
                    stack.push((path, next_remote));
                } else if path.is_file() {
                    files_to_upload.push((path, next_remote));
                }
            }
        }

        // Create all folders
        for dir in dirs_to_create {
            let _ = self.create_folder(&dir, true).await;
        }

        println!(
            "Found {} files to upload with {} concurrent threads...",
            files_to_upload.len().to_string().cyan(),
            threads.to_string().yellow()
        );

        let semaphore = Arc::new(Semaphore::new(threads.max(1)));
        let mut tasks = Vec::new();

        for (local_f, remote_f) in files_to_upload {
            let sem = semaphore.clone();
            let client = self.clone_instance();

            tasks.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                println!("=> Uploading: {} -> {}", local_f.display(), remote_f.cyan());
                client.upload_file(&local_f, &remote_f, true, 1).await
            }));
        }

        for task in tasks {
            task.await??;
        }

        Ok(())
    }

    pub async fn download_directory(
        &self,
        remote_dir: &str,
        local_dir: &Path,
        threads: usize,
    ) -> Result<()> {
        let norm_remote = Self::normalize_path(remote_dir);
        let item = self.get_item(&norm_remote).await?;
        if !item.is_dir() {
            bail!("'{}' is not a directory", remote_dir);
        }

        if !local_dir.exists() {
            std::fs::create_dir_all(local_dir)?;
        }

        let mut files_to_download: Vec<(String, PathBuf)> = Vec::new();
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
                    files_to_download.push((child_remote, child_local));
                }
            }
        }

        println!(
            "Found {} files to download with {} concurrent threads...",
            files_to_download.len().to_string().cyan(),
            threads.to_string().yellow()
        );

        let semaphore = Arc::new(Semaphore::new(threads.max(1)));
        let mut tasks = Vec::new();

        for (remote_f, local_f) in files_to_download {
            let sem = semaphore.clone();
            let client = self.clone_instance();

            tasks.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                println!(
                    "=> Downloading: {} -> {}",
                    remote_f.cyan(),
                    local_f.display()
                );
                client.download_file(&remote_f, &local_f, true).await
            }));
        }

        for task in tasks {
            task.await??;
        }

        Ok(())
    }

    pub async fn resume_task(&self, task: &TransferTask) -> Result<()> {
        let threads = task.threads.unwrap_or(4);
        match task.task_type {
            TaskType::Upload => {
                let local_p = Path::new(&task.local_path);
                if task.is_directory {
                    self.upload_directory(local_p, &task.remote_path, threads)
                        .await?;
                } else {
                    self.upload_file(local_p, &task.remote_path, true, threads)
                        .await?;
                }
            }
            TaskType::Download => {
                let local_p = PathBuf::from(&task.local_path);
                if task.is_directory {
                    self.download_directory(&task.remote_path, &local_p, threads)
                        .await?;
                } else {
                    self.download_file(&task.remote_path, &local_p, true)
                        .await?;
                }
            }
        }
        Ok(())
    }

    fn clone_instance(&self) -> Self {
        Self {
            http: self.http.clone(),
            auth_manager: self.auth_manager.clone(),
            config: self.config.clone(),
        }
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
        assert_eq!(
            OneDriveClient::normalize_path("/folder/subfolder"),
            "folder/subfolder"
        );
        assert_eq!(
            OneDriveClient::normalize_path("folder/subfolder/file.txt"),
            "folder/subfolder/file.txt"
        );
        assert_eq!(
            OneDriveClient::normalize_path("folder/sub/../file.txt"),
            "folder/file.txt"
        );
        assert_eq!(
            OneDriveClient::normalize_path(r"folder\sub\file.txt"),
            "folder/sub/file.txt"
        );
        assert_eq!(OneDriveClient::normalize_path("/a/b/../../c"), "c");
    }

    #[test]
    fn test_is_chunk_needed() {
        let missing = vec![(10485760, 20971519), (31457280, u64::MAX)];
        assert!(!OneDriveClient::is_chunk_needed(0, 10485759, &missing));
        assert!(OneDriveClient::is_chunk_needed(
            10485760, 20971519, &missing
        ));
        assert!(!OneDriveClient::is_chunk_needed(
            20971520, 31457279, &missing
        ));
        assert!(OneDriveClient::is_chunk_needed(
            31457280, 41943039, &missing
        ));
    }
}
