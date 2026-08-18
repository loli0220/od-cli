use crate::client::OneDriveClient;
use crate::config::Config;
use crate::ui::{print_drive_quota, print_item_info, print_items_table};
use anyhow::Result;
use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ReplSession {
    client: Arc<OneDriveClient>,
    config: Arc<Mutex<Config>>,
    current_dir: String,
}

impl ReplSession {
    pub fn new(
        client: Arc<OneDriveClient>,
        config: Arc<Mutex<Config>>,
    ) -> Self {
        Self {
            client,
            config,
            current_dir: "/".to_string(),
        }
    }

    pub fn resolve_path(&self, input: &str) -> String {
        let trimmed = input.trim();
        if trimmed.is_empty() || trimmed == "." {
            return self.current_dir.clone();
        }

        let raw = if trimmed.starts_with('/') || trimmed.starts_with('\\') {
            trimmed.to_string()
        } else if self.current_dir == "/" {
            format!("/{}", trimmed)
        } else {
            format!("{}/{}", self.current_dir, trimmed)
        };

        let norm = OneDriveClient::normalize_path(&raw);
        if norm.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", norm)
        }
    }

    fn history_path() -> Option<PathBuf> {
        Config::config_dir().ok().map(|d| d.join("history.txt"))
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut rl = DefaultEditor::new()?;
        let hist_path = Self::history_path();
        if let Some(ref p) = hist_path {
            let _ = rl.load_history(p);
        }

        println!();
        println!("{}", "=========================================================".cyan().bold());
        println!("{}", "     Welcome to OneDrive Interactive Shell (od-cli)     ".bright_white().bold());
        println!("{}", "=========================================================".cyan().bold());
        println!(
            "Type {} to see available commands, {} or {} to exit.",
            "help".yellow().bold(),
            "exit".yellow().bold(),
            "Ctrl+D".yellow().bold()
        );
        println!();

        loop {
            let user_info = {
                let conf = self.config.lock().await;
                conf.user_principal_name
                    .clone()
                    .unwrap_or_else(|| "guest".to_string())
            };

            let prompt = format!("od-cli [{}:{}]> ", user_info, self.current_dir);

            let readline = rl.readline(&prompt);
            match readline {
                Ok(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    let _ = rl.add_history_entry(trimmed);

                    let args = match shlex::split(trimmed) {
                        Some(a) if !a.is_empty() => a,
                        _ => continue,
                    };

                    let cmd = args[0].to_lowercase();
                    let rest = &args[1..];

                    match cmd.as_str() {
                        "exit" | "quit" | "q" => {
                            println!("{}", "Goodbye!".green());
                            break;
                        }
                        "clear" | "cls" => {
                            print!("\x1B[2J\x1B[1;1H");
                        }
                        "pwd" => {
                            println!("{}", self.current_dir.bright_green());
                        }
                        "cd" => {
                            let target = if rest.is_empty() { "/" } else { &rest[0] };
                            self.handle_cd(target).await;
                        }
                        "ls" | "dir" | "list" => {
                            self.handle_ls(rest).await;
                        }
                        "cat" => {
                            if rest.is_empty() {
                                println!("{}", "Usage: cat <remote_file_path>".yellow());
                            } else {
                                let resolved = self.resolve_path(&rest[0]);
                                if let Err(e) = self.client.cat_file(&resolved).await {
                                    eprintln!("{} {}", "Error:".red().bold(), e);
                                }
                                println!();
                            }
                        }
                        "mkdir" => {
                            if rest.is_empty() {
                                println!("{}", "Usage: mkdir <folder_path> [-p]".yellow());
                            } else {
                                let mut recursive = false;
                                let mut path_arg = None;
                                for arg in rest {
                                    if arg == "-p" || arg == "--parents" {
                                        recursive = true;
                                    } else if path_arg.is_none() {
                                        path_arg = Some(arg);
                                    }
                                }

                                if let Some(p) = path_arg {
                                    let resolved = self.resolve_path(p);
                                    match self.client.create_folder(&resolved, recursive).await {
                                        Ok(item) => println!("{} Created folder '{}'", "✓".green().bold(), item.name.cyan()),
                                        Err(e) => eprintln!("{} {}", "Error:".red().bold(), e),
                                    }
                                } else {
                                    println!("{}", "Usage: mkdir <folder_path> [-p]".yellow());
                                }
                            }
                        }
                        "upload" | "put" => {
                            self.handle_upload(rest).await;
                        }
                        "download" | "get" => {
                            self.handle_download(rest).await;
                        }
                        "rm" | "del" | "delete" => {
                            if rest.is_empty() {
                                println!("{}", "Usage: rm <remote_path>".yellow());
                            } else {
                                let resolved = self.resolve_path(&rest[0]);
                                match self.client.delete_item(&resolved).await {
                                    Ok(_) => println!("{} Deleted '{}'", "✓".green().bold(), resolved.red()),
                                    Err(e) => eprintln!("{} {}", "Error:".red().bold(), e),
                                }
                            }
                        }
                        "mv" | "move" | "rename" => {
                            if rest.len() < 2 {
                                println!("{}", "Usage: mv <source_path> <target_path>".yellow());
                            } else {
                                let src = self.resolve_path(&rest[0]);
                                let tgt = self.resolve_path(&rest[1]);
                                match self.client.move_item(&src, &tgt).await {
                                    Ok(item) => println!("{} Moved '{}' -> '{}'", "✓".green().bold(), src.dimmed(), item.name.cyan()),
                                    Err(e) => eprintln!("{} {}", "Error:".red().bold(), e),
                                }
                            }
                        }
                        "cp" | "copy" => {
                            if rest.len() < 2 {
                                println!("{}", "Usage: cp <source_path> <target_path>".yellow());
                            } else {
                                let src = self.resolve_path(&rest[0]);
                                let tgt = self.resolve_path(&rest[1]);
                                match self.client.copy_item(&src, &tgt).await {
                                    Ok(_) => println!("{} Copying '{}' to '{}' (async)", "✓".green().bold(), src.dimmed(), tgt.cyan()),
                                    Err(e) => eprintln!("{} {}", "Error:".red().bold(), e),
                                }
                            }
                        }
                        "info" | "stat" => {
                            let target = if rest.is_empty() {
                                self.current_dir.clone()
                            } else {
                                self.resolve_path(&rest[0])
                            };

                            if target == "/" {
                                match self.client.get_drive().await {
                                    Ok(drive) => print_drive_quota(&drive),
                                    Err(e) => eprintln!("{} {}", "Error:".red().bold(), e),
                                }
                            } else {
                                match self.client.get_item(&target).await {
                                    Ok(item) => print_item_info(&item),
                                    Err(e) => eprintln!("{} {}", "Error:".red().bold(), e),
                                }
                            }
                        }
                        "quota" | "df" => {
                            match self.client.get_drive().await {
                                Ok(drive) => print_drive_quota(&drive),
                                Err(e) => eprintln!("{} {}", "Error:".red().bold(), e),
                            }
                        }
                        "search" | "find" => {
                            if rest.is_empty() {
                                println!("{}", "Usage: search <keyword>".yellow());
                            } else {
                                let query = rest.join(" ");
                                match self.client.search(&query).await {
                                    Ok(items) => print_items_table(&items, true),
                                    Err(e) => eprintln!("{} {}", "Error:".red().bold(), e),
                                }
                            }
                        }
                        "share" => {
                            if rest.is_empty() {
                                println!("{}", "Usage: share <remote_path> [-t view|edit]".yellow());
                            } else {
                                let resolved = self.resolve_path(&rest[0]);
                                let link_type = if rest.iter().any(|a| a == "edit" || a == "-t edit") {
                                    "edit"
                                } else {
                                    "view"
                                };
                                match self.client.create_share_link(&resolved, link_type, Some("anonymous")).await {
                                    Ok(perm) => {
                                        if let Some(link) = perm.link
                                            && let Some(url) = link.web_url
                                        {
                                            println!("{} Share link created ({}):", "✓".green().bold(), link_type.cyan());
                                            println!("   {}", url.bright_blue().underline());
                                        }
                                    }
                                    Err(e) => eprintln!("{} {}", "Error:".red().bold(), e),
                                }
                            }
                        }
                        "whoami" | "status" => {
                            let conf = self.config.lock().await;
                            println!("Logged in user: {}", conf.user_principal_name.as_deref().unwrap_or("Unknown").bright_green());
                            println!("Display Name:   {}", conf.display_name.as_deref().unwrap_or("Unknown").yellow());
                            println!("Tenant ID:      {}", conf.get_tenant_id().dimmed());
                        }
                        "help" | "?" => {
                            self.print_help();
                        }
                        unknown => {
                            println!(
                                "Unknown command: '{}'. Type {} for a list of commands.",
                                unknown.red(),
                                "help".yellow()
                            );
                        }
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("(Use {} or Ctrl+D to exit)", "exit".yellow());
                }
                Err(ReadlineError::Eof) => {
                    println!("{}", "Goodbye!".green());
                    break;
                }
                Err(err) => {
                    eprintln!("{} {:?}", "Readline error:".red(), err);
                    break;
                }
            }
        }

        if let Some(ref p) = hist_path {
            let _ = rl.save_history(p);
        }

        Ok(())
    }

    async fn handle_cd(&mut self, target: &str) {
        let resolved = self.resolve_path(target);
        if resolved == "/" {
            self.current_dir = "/".to_string();
            return;
        }

        match self.client.get_item(&resolved).await {
            Ok(item) => {
                if item.is_dir() {
                    self.current_dir = resolved;
                } else {
                    println!("{} '{}' is not a folder.", "Error:".red(), resolved);
                }
            }
            Err(e) => {
                println!("{} {}", "Error:".red(), e);
            }
        }
    }

    async fn handle_ls(&self, args: &[String]) {
        let mut long_mode = false;
        let mut recursive = false;
        let mut target_path = None;

        for arg in args {
            if arg == "-l" || arg == "--long" {
                long_mode = true;
            } else if arg == "-r" || arg == "--recursive" {
                recursive = true;
            } else if target_path.is_none() && !arg.starts_with('-') {
                target_path = Some(arg.as_str());
            }
        }

        let resolved = match target_path {
            Some(p) => self.resolve_path(p),
            None => self.current_dir.clone(),
        };

        if recursive {
            println!("Recursively listing '{}':", resolved.cyan());
            self.list_recursive(&resolved, 0, long_mode).await;
        } else {
            match self.client.list_children(&resolved).await {
                Ok(items) => print_items_table(&items, long_mode),
                Err(e) => eprintln!("{} {}", "Error:".red().bold(), e),
            }
        }
    }

    fn list_recursive<'a>(
        &'a self,
        path: &'a str,
        depth: usize,
        long_mode: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>> {
        Box::pin(async move {
            match self.client.list_children(path).await {
                Ok(items) => {
                    let indent = "  ".repeat(depth);
                    for item in &items {
                        if item.is_dir() {
                            println!("{}{}/", indent, item.name.cyan().bold());
                            let child_path = if path == "/" {
                                format!("/{}", item.name)
                            } else {
                                format!("{}/{}", path, item.name)
                            };
                            self.list_recursive(&child_path, depth + 1, long_mode).await;
                        } else {
                            let size = item.size.unwrap_or(0);
                            if long_mode {
                                println!(
                                    "{}{:<30} {:>10}  {}",
                                    indent,
                                    item.name,
                                    crate::ui::format_size(size).dimmed(),
                                    item.id.dimmed()
                                );
                            } else {
                                println!("{}{:<30} {:>10}", indent, item.name, crate::ui::format_size(size).dimmed());
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("{} Failed to list '{}': {}", "Error:".red(), path, e);
                }
            }
        })
    }

    async fn handle_upload(&self, args: &[String]) {
        if args.is_empty() {
            println!("{}", "Usage: upload <local_path> [remote_path] [-r]".yellow());
            return;
        }

        let mut recursive = false;
        let mut positional = Vec::new();

        for arg in args {
            if arg == "-r" || arg == "--recursive" {
                recursive = true;
            } else {
                positional.push(arg.as_str());
            }
        }

        if positional.is_empty() {
            println!("{}", "Usage: upload <local_path> [remote_path] [-r]".yellow());
            return;
        }

        let local_p = Path::new(positional[0]);
        let remote_target = if positional.len() > 1 {
            self.resolve_path(positional[1])
        } else {
            self.current_dir.clone()
        };

        if local_p.is_dir() || recursive {
            println!("Uploading local directory {:?} to remote '{}'...", local_p, remote_target);
            if let Err(e) = self.client.upload_directory(local_p, &remote_target).await {
                eprintln!("{} {}", "Upload failed:".red().bold(), e);
            } else {
                println!("{} Directory upload complete!", "✓".green().bold());
            }
        } else {
            match self.client.upload_file(local_p, &remote_target, true).await {
                Ok(item) => println!("{} Uploaded '{}' successfully.", "✓".green().bold(), item.name.cyan()),
                Err(e) => eprintln!("{} {}", "Upload failed:".red().bold(), e),
            }
        }
    }

    async fn handle_download(&self, args: &[String]) {
        if args.is_empty() {
            println!("{}", "Usage: download <remote_path> [local_path] [-r]".yellow());
            return;
        }

        let mut recursive = false;
        let mut positional = Vec::new();

        for arg in args {
            if arg == "-r" || arg == "--recursive" {
                recursive = true;
            } else {
                positional.push(arg.as_str());
            }
        }

        if positional.is_empty() {
            println!("{}", "Usage: download <remote_path> [local_path] [-r]".yellow());
            return;
        }

        let remote_src = self.resolve_path(positional[0]);
        let local_dst = if positional.len() > 1 {
            PathBuf::from(positional[1])
        } else {
            PathBuf::from(".")
        };

        let is_dir_check = self.client.get_item(&remote_src).await.map(|i| i.is_dir()).unwrap_or(false);

        if is_dir_check || recursive {
            println!("Downloading remote folder '{}' to {:?}...", remote_src, local_dst);
            if let Err(e) = self.client.download_directory(&remote_src, &local_dst).await {
                eprintln!("{} {}", "Download failed:".red().bold(), e);
            } else {
                println!("{} Directory download complete!", "✓".green().bold());
            }
        } else {
            if let Err(e) = self.client.download_file(&remote_src, &local_dst, true).await {
                eprintln!("{} {}", "Download failed:".red().bold(), e);
            } else {
                println!("{} Download complete.", "✓".green().bold());
            }
        }
    }

    fn print_help(&self) {
        println!("{}", "Available Interactive Commands:".cyan().bold());
        println!("  {:<26} List files and folders in directory", "ls [-l] [-r] [path]".bright_yellow());
        println!("  {:<26} Change current working directory", "cd <path>".bright_yellow());
        println!("  {:<26} Print current working directory", "pwd".bright_yellow());
        println!("  {:<26} Create a folder (-p for recursive)", "mkdir <folder> [-p]".bright_yellow());
        println!("  {:<26} Print file contents to stdout", "cat <file>".bright_yellow());
        println!("  {:<26} Upload file or folder (put)", "upload <local> [remote] [-r]".bright_yellow());
        println!("  {:<26} Download file or folder (get)", "download <remote> [local] [-r]".bright_yellow());
        println!("  {:<26} Delete a file or folder", "rm <path>".bright_yellow());
        println!("  {:<26} Move or rename file/folder", "mv <src> <tgt>".bright_yellow());
        println!("  {:<26} Copy file/folder", "cp <src> <tgt>".bright_yellow());
        println!("  {:<26} View item metadata or drive quota", "info [path]".bright_yellow());
        println!("  {:<26} View drive storage quota", "quota".bright_yellow());
        println!("  {:<26} Search files across OneDrive", "search <keyword>".bright_yellow());
        println!("  {:<26} Create shareable link", "share <path> [-t view|edit]".bright_yellow());
        println!("  {:<26} Show current user info", "whoami".bright_yellow());
        println!("  {:<26} Clear screen", "clear".bright_yellow());
        println!("  {:<26} Show this help", "help".bright_yellow());
        println!("  {:<26} Exit interactive shell", "exit / quit".bright_yellow());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthManager;

    #[test]
    fn test_resolve_path_cases() {
        // We can test resolve_path logic directly by instantiating ReplSession with mock objects or constructing
        let client = Arc::new(OneDriveClient::new(
            Arc::new(AuthManager::new()),
            Arc::new(Mutex::new(Config::default())),
        ));
        let config = Arc::new(Mutex::new(Config::default()));

        let mut session = ReplSession::new(client, config);
        assert_eq!(session.resolve_path(""), "/");
        assert_eq!(session.resolve_path("."), "/");
        assert_eq!(session.resolve_path("photos"), "/photos");
        assert_eq!(session.resolve_path("/documents/2026"), "/documents/2026");

        session.current_dir = "/documents".to_string();
        assert_eq!(session.resolve_path("report.pdf"), "/documents/report.pdf");
        assert_eq!(session.resolve_path(".."), "/");
        assert_eq!(session.resolve_path("/root_folder"), "/root_folder");
        assert_eq!(session.resolve_path("sub/file.txt"), "/documents/sub/file.txt");
    }
}

