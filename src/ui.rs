use crate::types::{Drive, DriveItem};
use chrono::DateTime;
use colored::Colorize;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table};
use indicatif::{ProgressBar, ProgressStyle};

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_datetime(iso_str: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(iso_str) {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        iso_str.to_string()
    }
}

pub fn print_items_table(items: &[DriveItem], long_mode: bool) {
    if items.is_empty() {
        println!("{}", "  (empty directory)".italic().dimmed());
        return;
    }

    let mut table = Table::new();
    table
        .load_style(UTF8_FULL.with_rounded_corners())
        .set_content_arrangement(ContentArrangement::Dynamic);

    if long_mode {
        table.set_header(vec![
            Cell::new("Type").set_alignment(CellAlignment::Center),
            Cell::new("Name"),
            Cell::new("Size").set_alignment(CellAlignment::Right),
            Cell::new("Modified"),
            Cell::new("ID"),
        ]);

        for item in items {
            let type_cell = if item.is_dir() {
                Cell::new("DIR").set_alignment(CellAlignment::Center)
            } else {
                Cell::new("FILE").set_alignment(CellAlignment::Center)
            };

            let name_cell = if item.is_dir() {
                Cell::new(format!("{}/", item.name))
            } else {
                Cell::new(&item.name)
            };

            let size_cell = if item.is_dir() {
                let count = item.folder.as_ref().and_then(|f| f.child_count).unwrap_or(0);
                Cell::new(format!("{} items", count)).set_alignment(CellAlignment::Right)
            } else {
                let s = item.size.unwrap_or(0);
                Cell::new(format_size(s)).set_alignment(CellAlignment::Right)
            };

            let mod_time = item
                .last_modified_date_time
                .as_deref()
                .map(format_datetime)
                .unwrap_or_else(|| "-".to_string());

            table.add_row(vec![
                type_cell,
                name_cell,
                size_cell,
                Cell::new(mod_time),
                Cell::new(&item.id),
            ]);
        }
    } else {
        table.set_header(vec![
            Cell::new("Type").set_alignment(CellAlignment::Center),
            Cell::new("Name"),
            Cell::new("Size").set_alignment(CellAlignment::Right),
            Cell::new("Modified"),
        ]);

        for item in items {
            let type_cell = if item.is_dir() {
                Cell::new("DIR").set_alignment(CellAlignment::Center)
            } else {
                Cell::new("FILE").set_alignment(CellAlignment::Center)
            };

            let name_cell = if item.is_dir() {
                Cell::new(format!("{}/", item.name))
            } else {
                Cell::new(&item.name)
            };

            let size_cell = if item.is_dir() {
                let count = item.folder.as_ref().and_then(|f| f.child_count).unwrap_or(0);
                Cell::new(format!("{} items", count)).set_alignment(CellAlignment::Right)
            } else {
                let s = item.size.unwrap_or(0);
                Cell::new(format_size(s)).set_alignment(CellAlignment::Right)
            };

            let mod_time = item
                .last_modified_date_time
                .as_deref()
                .map(format_datetime)
                .unwrap_or_else(|| "-".to_string());

            table.add_row(vec![
                type_cell,
                name_cell,
                size_cell,
                Cell::new(mod_time),
            ]);
        }
    }

    println!("{table}");
    println!("Total: {} items", items.len().to_string().cyan());
}

pub fn print_drive_quota(drive: &Drive) {
    println!("{}", "=== OneDrive Storage Quota ===".bold().cyan());
    println!("Drive ID:   {}", drive.id.dimmed());
    if let Some(ref drive_type) = drive.drive_type {
        println!("Drive Type: {}", drive_type.yellow());
    }
    if let Some(ref owner) = drive.owner
        && let Some(ref user) = owner.user
        && let Some(ref name) = user.display_name
    {
        println!("Owner:      {}", name.bright_green());
    }

    if let Some(ref quota) = drive.quota {
        let total = quota.total.unwrap_or(0);
        let used = quota.used.unwrap_or(0);
        let remaining = quota.remaining.unwrap_or(0);
        let deleted = quota.deleted.unwrap_or(0);

        println!("--------------------------------------------------");
        println!("Total Space:     {:>12}", format_size(total).bold());
        println!("Used Space:      {:>12}", format_size(used).red());
        println!("Remaining Space: {:>12}", format_size(remaining).green());
        println!("Recycle Bin:     {:>12}", format_size(deleted).yellow());

        if total > 0 {
            let pct = (used as f64 / total as f64) * 100.0;
            let bar_len: usize = 30;
            let filled = ((pct / 100.0 * bar_len as f64).round() as usize).min(bar_len);
            let empty = bar_len.saturating_sub(filled);

            let bar = format!(
                "[{}{}] {:.1}%",
                "=".repeat(filled).red(),
                "-".repeat(empty).dimmed(),
                pct
            );
            println!("Usage:           {bar}");
        }
    }
}

pub fn print_item_info(item: &DriveItem) {
    println!("{}", "=== Item Details ===".bold().cyan());
    println!("Name:       {}", item.name.bold());
    println!("ID:         {}", item.id.dimmed());
    println!(
        "Type:       {}",
        if item.is_dir() {
            "Folder".cyan().bold()
        } else {
            "File".green()
        }
    );

    if let Some(size) = item.size {
        println!("Size:       {}", format_size(size));
    }

    if let Some(ref folder) = item.folder
        && let Some(count) = folder.child_count
    {
        println!("Children:   {count} items");
    }

    if let Some(ref file) = item.file {
        if let Some(ref mime) = file.mime_type {
            println!("MIME Type:  {mime}");
        }
        if let Some(ref hashes) = file.hashes {
            if let Some(ref sha1) = hashes.sha1_hash {
                println!("SHA1 Hash:  {}", sha1.dimmed());
            }
            if let Some(ref qxor) = hashes.quick_xor_hash {
                println!("QuickXOR:   {}", qxor.dimmed());
            }
            if let Some(ref crc32) = hashes.crc32_hash {
                println!("CRC32:      {}", crc32.dimmed());
            }
        }
    }

    if let Some(ref created) = item.created_date_time {
        println!("Created:    {}", format_datetime(created));
    }
    if let Some(ref modified) = item.last_modified_date_time {
        println!("Modified:   {}", format_datetime(modified));
    }
    if let Some(ref web_url) = item.web_url {
        println!("Web URL:    {}", web_url.bright_blue().underline());
    }
    if let Some(ref parent) = item.parent_reference
        && let Some(ref path) = parent.path
    {
        println!("Parent:     {}", path.dimmed());
    }
}

pub fn create_upload_progress(total_bytes: u64, message: &str) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta}) {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("#>-"),
    );
    pb.set_message(message.to_string());
    pb
}

pub fn create_download_progress(total_bytes: u64, message: &str) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.green/white}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta}) {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-"),
    );
    pb.set_message(message.to_string());
    pb
}

pub fn print_tasks_table(tasks: &[crate::tasks::TransferTask]) {
    if tasks.is_empty() {
        println!("{}", "  (no transfer tasks recorded)".italic().dimmed());
        return;
    }

    let mut table = Table::new();
    table
        .load_style(UTF8_FULL.with_rounded_corners())
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("ID").set_alignment(CellAlignment::Center),
        Cell::new("Type").set_alignment(CellAlignment::Center),
        Cell::new("Status").set_alignment(CellAlignment::Center),
        Cell::new("Local Path"),
        Cell::new("Remote Path"),
        Cell::new("Progress").set_alignment(CellAlignment::Right),
        Cell::new("Updated"),
    ]);

    for task in tasks {
        let type_cell = match task.task_type {
            crate::tasks::TaskType::Upload => Cell::new("Upload").set_alignment(CellAlignment::Center),
            crate::tasks::TaskType::Download => Cell::new("Download").set_alignment(CellAlignment::Center),
        };

        let status_str = match task.status {
            crate::tasks::TaskStatus::Running => "Running".cyan().bold().to_string(),
            crate::tasks::TaskStatus::Interrupted => "Interrupted".yellow().bold().to_string(),
            crate::tasks::TaskStatus::Failed => "Failed".red().bold().to_string(),
            crate::tasks::TaskStatus::Completed => "Completed".green().to_string(),
            crate::tasks::TaskStatus::Pending => "Pending".dimmed().to_string(),
        };

        let progress_str = if task.total_size > 0 {
            let pct = (task.transferred_bytes as f64 / task.total_size as f64) * 100.0;
            format!("{}/{} ({:.1}%)", format_size(task.transferred_bytes), format_size(task.total_size), pct)
        } else {
            format_size(task.transferred_bytes)
        };

        let dt = chrono::DateTime::from_timestamp(task.updated_at, 0)
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "-".to_string());

        table.add_row(vec![
            Cell::new(&task.id).set_alignment(CellAlignment::Center),
            type_cell,
            Cell::new(status_str).set_alignment(CellAlignment::Center),
            Cell::new(&task.local_path),
            Cell::new(&task.remote_path),
            Cell::new(progress_str).set_alignment(CellAlignment::Right),
            Cell::new(dt),
        ]);
    }

    println!("{table}");
    println!("Total: {} tasks", tasks.len().to_string().cyan());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_size(1024 * 1024 * 1024 * 1024), "1.00 TB");
    }

    #[test]
    fn test_format_datetime() {
        let iso = "2026-08-17T23:30:00Z";
        let formatted = format_datetime(iso);
        assert!(formatted.contains("2026-08-17"));
    }
}
