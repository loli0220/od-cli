use crate::config::Config;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskType {
    Upload,
    Download,
}

impl std::fmt::Display for TaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskType::Upload => write!(f, "Upload"),
            TaskType::Download => write!(f, "Download"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Running,
    Interrupted,
    Failed,
    Completed,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "Pending"),
            TaskStatus::Running => write!(f, "Running"),
            TaskStatus::Interrupted => write!(f, "Interrupted"),
            TaskStatus::Failed => write!(f, "Failed"),
            TaskStatus::Completed => write!(f, "Completed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferTask {
    pub id: String,
    pub task_type: TaskType,
    pub local_path: String,
    pub remote_path: String,
    pub total_size: u64,
    pub transferred_bytes: u64,
    pub is_directory: bool,
    pub threads: Option<usize>,
    pub status: TaskStatus,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TaskStore {
    pub tasks: HashMap<String, TransferTask>,
    pub next_id: u64,
}

impl TaskStore {
    fn tasks_path() -> Result<PathBuf> {
        let dir = Config::config_dir()?;
        Ok(dir.join("tasks.json"))
    }

    pub fn load() -> Self {
        if let Ok(path) = Self::tasks_path()
            && path.exists()
            && let Ok(content) = fs::read_to_string(&path)
            && let Ok(store) = serde_json::from_str::<TaskStore>(&content)
        {
            return store;
        }
        Self {
            tasks: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::tasks_path()?;
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize task store")?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write tasks to {}", path.display()))?;
        Ok(())
    }

    pub fn create_or_get_task(
        &mut self,
        task_type: TaskType,
        local_path: &Path,
        remote_path: &str,
        total_size: u64,
        is_directory: bool,
        threads: Option<usize>,
    ) -> String {
        let local_str = local_path.to_string_lossy().to_string();
        let remote_str = crate::client::OneDriveClient::normalize_path(remote_path);

        // Check if an identical uncompleted task exists
        for task in self.tasks.values_mut() {
            if task.task_type == task_type
                && task.local_path == local_str
                && task.remote_path == remote_str
                && task.status != TaskStatus::Completed
            {
                task.status = TaskStatus::Running;
                task.updated_at = Utc::now().timestamp();
                let id = task.id.clone();
                let _ = self.save();
                return id;
            }
        }

        let id = self.next_id.to_string();
        self.next_id += 1;
        let now = Utc::now().timestamp();

        let task = TransferTask {
            id: id.clone(),
            task_type,
            local_path: local_str,
            remote_path: remote_str,
            total_size,
            transferred_bytes: 0,
            is_directory,
            threads,
            status: TaskStatus::Running,
            error_message: None,
            created_at: now,
            updated_at: now,
        };

        self.tasks.insert(id.clone(), task);
        let _ = self.save();
        id
    }

    #[allow(dead_code)]
    pub fn update_progress(&mut self, task_id: &str, transferred: u64) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.transferred_bytes = transferred;
            task.updated_at = Utc::now().timestamp();
            let _ = self.save();
        }
    }

    pub fn mark_completed(&mut self, task_id: &str) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = TaskStatus::Completed;
            task.transferred_bytes = task.total_size;
            task.updated_at = Utc::now().timestamp();
            let _ = self.save();
        }
    }

    pub fn mark_interrupted(&mut self, task_id: &str, err: Option<String>, transferred: u64) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = TaskStatus::Interrupted;
            task.error_message = err;
            task.transferred_bytes = transferred;
            task.updated_at = Utc::now().timestamp();
            let _ = self.save();
        }
    }

    #[allow(dead_code)]
    pub fn mark_failed(&mut self, task_id: &str, err: String) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = TaskStatus::Failed;
            task.error_message = Some(err);
            task.updated_at = Utc::now().timestamp();
            let _ = self.save();
        }
    }

    pub fn list(&self) -> Vec<TransferTask> {
        let mut list: Vec<TransferTask> = self.tasks.values().cloned().collect();
        list.sort_by_key(|t| t.id.parse::<u64>().unwrap_or(0));
        list
    }

    pub fn list_resumable(&self) -> Vec<TransferTask> {
        let mut list: Vec<TransferTask> = self
            .tasks
            .values()
            .filter(|t| matches!(t.status, TaskStatus::Interrupted | TaskStatus::Failed | TaskStatus::Pending))
            .cloned()
            .collect();
        list.sort_by_key(|t| t.id.parse::<u64>().unwrap_or(0));
        list
    }

    pub fn get(&self, id: &str) -> Option<&TransferTask> {
        self.tasks.get(id)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let removed = self.tasks.remove(id).is_some();
        if removed {
            let _ = self.save();
        }
        removed
    }

    pub fn clear(&mut self) {
        self.tasks.clear();
        let _ = self.save();
    }

    pub fn clean_completed(&mut self) {
        self.tasks.retain(|_, t| t.status != TaskStatus::Completed);
        let _ = self.save();
    }
}
