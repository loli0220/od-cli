mod auth;
mod cli;
mod client;
mod config;
mod repl;
mod sessions;
mod tasks;
mod types;
mod ui;

use anyhow::Result;
use clap::Parser;
use cli::{AuthAction, Cli, Commands, ConfigAction, TasksAction};
use colored::Colorize;
use config::Config;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config_val = Config::load()?;
    let default_threads = config_val.get_threads();
    let ip_pref_override = if cli.ipv4 {
        Some("ipv4".to_string())
    } else if cli.ipv6 {
        Some("ipv6".to_string())
    } else {
        config_val
            .get_ip_preference()
            .map(std::string::ToString::to_string)
    };

    let config = Arc::new(Mutex::new(config_val));
    let auth_manager = Arc::new(auth::AuthManager::new(ip_pref_override.as_deref()));
    let client = Arc::new(client::OneDriveClient::new(
        auth_manager.clone(),
        config.clone(),
        ip_pref_override.as_deref(),
    ));

    match cli.command {
        None | Some(Commands::Shell) => {
            // Interactive REPL Shell Mode
            let mut session = repl::ReplSession::new(client, config);
            session.run().await?;
        }

        Some(Commands::Auth(args)) => match args.action {
            AuthAction::Login => {
                let mut conf = config.lock().await;
                auth_manager.login(&mut conf).await?;
            }
            AuthAction::Logout => {
                let mut conf = config.lock().await;
                conf.clear_tokens();
                conf.save()?;
                println!("{}", "Successfully logged out. Tokens cleared.".green());
            }
            AuthAction::Status => {
                let conf = config.lock().await;
                if conf.access_token.is_none() {
                    println!(
                        "{}",
                        "Not logged in. Run `od-cli auth login` to sign in.".yellow()
                    );
                } else {
                    println!("{}", "=== Authentication Status ===".bold().cyan());
                    println!(
                        "User:         {}",
                        conf.user_principal_name
                            .as_deref()
                            .unwrap_or("Unknown")
                            .bright_green()
                    );
                    println!(
                        "Display Name: {}",
                        conf.display_name.as_deref().unwrap_or("Unknown").yellow()
                    );
                    println!("Tenant:       {}", conf.get_tenant_id().dimmed());
                    println!("Client ID:    {}", conf.get_client_id().dimmed());
                    println!(
                        "Token Status: {}",
                        if conf.is_token_expired() {
                            "Expired (auto-refresh on next request)".yellow()
                        } else {
                            "Valid".green()
                        }
                    );
                }
            }
        },

        Some(Commands::Config(args)) => match args.action {
            ConfigAction::Set { key, value } => {
                let mut conf = config.lock().await;
                match key.to_lowercase().as_str() {
                    "client_id" => {
                        conf.client_id = Some(value.clone());
                        println!("Set {} = {}", "client_id".cyan(), value);
                    }
                    "tenant_id" => {
                        conf.tenant_id = Some(value.clone());
                        println!("Set {} = {}", "tenant_id".cyan(), value);
                    }
                    "chunk_size_mb" => {
                        let mb: usize = value
                            .parse()
                            .map_err(|_| anyhow::anyhow!("Invalid number for chunk_size_mb"))?;
                        conf.chunk_size_mb = Some(mb);
                        println!("Set {} = {} MB", "chunk_size_mb".cyan(), mb);
                    }
                    "ip_preference" | "ip_version" | "ip" => {
                        let val = match value.to_lowercase().as_str() {
                            "ipv4" | "v4" | "4" => "ipv4",
                            "ipv6" | "v6" | "6" => "ipv6",
                            _ => "auto",
                        };
                        conf.ip_preference = Some(val.to_string());
                        println!("Set {} = {}", "ip_preference".cyan(), val);
                    }
                    "threads" | "concurrency" | "jobs" => {
                        let th: usize = value
                            .parse()
                            .map_err(|_| anyhow::anyhow!("Invalid number for threads"))?;
                        conf.threads = Some(th.max(1));
                        println!("Set {} = {}", "threads".cyan(), th.max(1));
                    }
                    other => {
                        eprintln!(
                            "{} Unknown configuration key '{}'. Valid keys: client_id, tenant_id, chunk_size_mb, ip_preference, threads",
                            "Error:".red(),
                            other
                        );
                        return Ok(());
                    }
                }
                conf.save()?;
            }
            ConfigAction::Get { key } => {
                let conf = config.lock().await;
                match key.to_lowercase().as_str() {
                    "client_id" => println!("{}", conf.get_client_id()),
                    "tenant_id" => println!("{}", conf.get_tenant_id()),
                    "chunk_size_mb" => println!(
                        "{}",
                        conf.chunk_size_mb.unwrap_or(config::DEFAULT_CHUNK_SIZE_MB)
                    ),
                    "ip_preference" | "ip_version" | "ip" => {
                        println!("{}", conf.ip_preference.as_deref().unwrap_or("auto"))
                    }
                    "threads" | "concurrency" | "jobs" => println!("{}", conf.get_threads()),
                    other => {
                        eprintln!("Unknown configuration key: {}", other.red());
                    }
                }
            }
            ConfigAction::Show => {
                let conf = config.lock().await;
                println!("{}", "=== od-cli Configuration ===".bold().cyan());
                println!(
                    "Config Path:   {}",
                    Config::config_path()?.display().to_string().dimmed()
                );
                println!("Client ID:     {}", conf.get_client_id().bright_green());
                println!("Tenant ID:     {}", conf.get_tenant_id().bright_green());
                println!(
                    "Chunk Size:    {} MB",
                    conf.chunk_size_mb
                        .unwrap_or(config::DEFAULT_CHUNK_SIZE_MB)
                        .to_string()
                        .bright_yellow()
                );
                println!(
                    "IP Preference: {}",
                    conf.ip_preference.as_deref().unwrap_or("auto").cyan()
                );
                println!(
                    "Threads:       {}",
                    conf.get_threads().to_string().bright_yellow()
                );
                println!(
                    "Logged In:     {}",
                    if conf.access_token.is_some() {
                        "Yes".green()
                    } else {
                        "No".red()
                    }
                );
                if let Some(ref email) = conf.user_principal_name {
                    println!("Account:       {}", email.cyan());
                }
            }
            ConfigAction::Path => {
                println!("{}", Config::config_path()?.display());
            }
        },

        Some(Commands::Ls(args)) => {
            let items = client.list_children(&args.path).await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else if args.recursive {
                list_recursive_cli(&client, &args.path, 0, args.long).await?;
            } else {
                ui::print_items_table(&items, args.long);
            }
        }

        Some(Commands::Info(args)) => {
            let norm = client::OneDriveClient::normalize_path(&args.path);
            if norm.is_empty() {
                let drive = client.get_drive().await?;
                if args.json {
                    println!("{}", serde_json::to_string_pretty(&drive)?);
                } else {
                    ui::print_drive_quota(&drive);
                }
            } else {
                let item = client.get_item(&norm).await?;
                if args.json {
                    println!("{}", serde_json::to_string_pretty(&item)?);
                } else {
                    ui::print_item_info(&item);
                }
            }
        }

        Some(Commands::Quota) => {
            let drive = client.get_drive().await?;
            ui::print_drive_quota(&drive);
        }

        Some(Commands::Mkdir(args)) => {
            let item = client.create_folder(&args.path, args.parents).await?;
            println!(
                "{} Created folder '{}' (ID: {})",
                "✓".green().bold(),
                item.name.cyan(),
                item.id.dimmed()
            );
        }

        Some(Commands::Upload(args)) => {
            let local_p = Path::new(&args.local_path);
            let remote_dest = args.remote_path.as_deref().unwrap_or("");
            let threads = args.threads.or(cli.threads).unwrap_or(default_threads);

            if local_p.is_dir() || args.recursive {
                println!(
                    "Uploading directory {} -> '{}' (threads: {})...",
                    local_p.display(),
                    remote_dest,
                    threads
                );
                client
                    .upload_directory(local_p, remote_dest, threads)
                    .await?;
                println!("{} Directory upload complete!", "✓".green().bold());
            } else {
                let item = client
                    .upload_file(local_p, remote_dest, true, threads)
                    .await?;
                println!(
                    "{} Uploaded '{}' (Size: {})",
                    "✓".green().bold(),
                    item.name.cyan(),
                    ui::format_size(item.size.unwrap_or(0))
                );
            }
        }

        Some(Commands::Download(args)) => {
            let local_dst = PathBuf::from(&args.local_path);
            let is_dir = client
                .get_item(&args.remote_path)
                .await
                .is_ok_and(|i| i.is_dir());
            let threads = args.threads.or(cli.threads).unwrap_or(default_threads);

            if is_dir || args.recursive {
                println!(
                    "Downloading directory '{}' -> {} (threads: {})...",
                    args.remote_path,
                    local_dst.display(),
                    threads
                );
                client
                    .download_directory(&args.remote_path, &local_dst, threads)
                    .await?;
                println!("{} Directory download complete!", "✓".green().bold());
            } else {
                client
                    .download_file(&args.remote_path, &local_dst, true)
                    .await?;
                println!("{} Download complete.", "✓".green().bold());
            }
        }

        Some(Commands::Cat(args)) => {
            client.cat_file(&args.path).await?;
        }

        Some(Commands::Rm(args)) => {
            client.delete_item(&args.path).await?;
            println!("{} Deleted '{}'", "✓".green().bold(), args.path.red());
        }

        Some(Commands::Mv(args)) => {
            let item = client.move_item(&args.source, &args.target).await?;
            println!(
                "{} Moved '{}' -> '{}'",
                "✓".green().bold(),
                args.source.dimmed(),
                item.name.cyan()
            );
        }

        Some(Commands::Cp(args)) => {
            client.copy_item(&args.source, &args.target).await?;
            println!(
                "{} Initiated copy '{}' -> '{}'",
                "✓".green().bold(),
                args.source.dimmed(),
                args.target.cyan()
            );
        }

        Some(Commands::Search(args)) => {
            let results = client.search(&args.query).await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                ui::print_items_table(&results, true);
            }
        }

        Some(Commands::Share(args)) => {
            let perm = client
                .create_share_link(
                    &args.path,
                    &args.link_type.to_string(),
                    Some(&args.scope.to_string()),
                )
                .await?;

            if let Some(link) = perm.link
                && let Some(url) = link.web_url
            {
                println!(
                    "{} Share link created successfully ({}):",
                    "✓".green().bold(),
                    args.link_type.to_string().cyan()
                );
                println!("   {}", url.bright_blue().underline());
            }
        }

        Some(Commands::Tasks(args)) => {
            let action = args.action.unwrap_or(TasksAction::List);
            match action {
                TasksAction::List => {
                    let store = tasks::TaskStore::load();
                    println!("{}", "=== Transfer Task Queue ===".bold().cyan());
                    ui::print_tasks_table(&store.list());
                }
                TasksAction::Resume { id } => {
                    let store = tasks::TaskStore::load();
                    let tasks_to_resume: Vec<tasks::TransferTask> = match id.as_deref() {
                        Some("all") | None => store.list_resumable(),
                        Some(id_str) => {
                            if let Some(t) = store.get(id_str) {
                                vec![t.clone()]
                            } else {
                                eprintln!(
                                    "{} Task with ID '{}' not found.",
                                    "Error:".red(),
                                    id_str
                                );
                                return Ok(());
                            }
                        }
                    };

                    if tasks_to_resume.is_empty() {
                        println!("{}", "No interrupted or pending tasks to resume.".green());
                        return Ok(());
                    }

                    println!(
                        "⚡ Resuming {} transfer task(s)...",
                        tasks_to_resume.len().to_string().cyan()
                    );

                    for task in tasks_to_resume {
                        println!(
                            "=> Resuming task [{}]: {} ({}) -> {}",
                            task.id.yellow(),
                            task.task_type.to_string().cyan(),
                            task.local_path,
                            task.remote_path.cyan()
                        );
                        if let Err(e) = client.resume_task(&task).await {
                            eprintln!("{} Task [{}] failed: {}", "Error:".red(), task.id, e);
                        } else {
                            println!(
                                "{} Task [{}] completed successfully!",
                                "✓".green().bold(),
                                task.id
                            );
                        }
                    }
                }
                TasksAction::Rm { id } => {
                    let mut store = tasks::TaskStore::load();
                    if store.remove(&id) {
                        println!("{} Removed task [{}]", "✓".green().bold(), id.yellow());
                    } else {
                        eprintln!("{} Task with ID '{}' not found.", "Error:".red(), id);
                    }
                }
                TasksAction::Clear => {
                    let mut store = tasks::TaskStore::load();
                    store.clear();
                    println!("{} Cleared all transfer tasks.", "✓".green().bold());
                }
                TasksAction::Clean => {
                    let mut store = tasks::TaskStore::load();
                    store.clean_completed();
                    println!("{} Cleaned up all completed tasks.", "✓".green().bold());
                }
            }
        }
    }

    Ok(())
}

fn list_recursive_cli<'a>(
    client: &'a client::OneDriveClient,
    path: &'a str,
    depth: usize,
    long_mode: bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + 'a>> {
    Box::pin(async move {
        let items = client.list_children(path).await?;
        let indent = "  ".repeat(depth);
        for item in items {
            if item.is_dir() {
                println!("{}{}/", indent, item.name.cyan().bold());
                let child_path = if path == "/" || path.is_empty() {
                    format!("/{}", item.name)
                } else {
                    format!("{}/{}", path, item.name)
                };
                list_recursive_cli(client, &child_path, depth + 1, long_mode).await?;
            } else {
                let size = item.size.unwrap_or(0);
                if long_mode {
                    println!(
                        "{}{:<30} {:>10}  {}",
                        indent,
                        item.name,
                        ui::format_size(size).dimmed(),
                        item.id.dimmed()
                    );
                } else {
                    println!(
                        "{}{:<30} {:>10}",
                        indent,
                        item.name,
                        ui::format_size(size).dimmed()
                    );
                }
            }
        }
        Ok(())
    })
}
