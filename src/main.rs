mod auth;
mod cli;
mod client;
mod config;
mod repl;
mod types;
mod ui;

use anyhow::Result;
use clap::Parser;
use cli::{AuthAction, Cli, Commands, ConfigAction};
use colored::Colorize;
use config::Config;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = Arc::new(Mutex::new(Config::load()?));
    let auth_manager = Arc::new(auth::AuthManager::new());
    let client = Arc::new(client::OneDriveClient::new(
        auth_manager.clone(),
        config.clone(),
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
                    println!("{}", "Not logged in. Run `od-cli auth login` to sign in.".yellow());
                } else {
                    println!("{}", "=== Authentication Status ===".bold().cyan());
                    println!("User:         {}", conf.user_principal_name.as_deref().unwrap_or("Unknown").bright_green());
                    println!("Display Name: {}", conf.display_name.as_deref().unwrap_or("Unknown").yellow());
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
                        let mb: usize = value.parse().map_err(|_| anyhow::anyhow!("Invalid number for chunk_size_mb"))?;
                        conf.chunk_size_mb = Some(mb);
                        println!("Set {} = {} MB", "chunk_size_mb".cyan(), mb);
                    }
                    other => {
                        eprintln!(
                            "{} Unknown configuration key '{}'. Valid keys: client_id, tenant_id, chunk_size_mb",
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
                    "chunk_size_mb" => println!("{}", conf.chunk_size_mb.unwrap_or(config::DEFAULT_CHUNK_SIZE_MB)),
                    other => {
                        eprintln!("Unknown configuration key: {}", other.red());
                    }
                }
            }
            ConfigAction::Show => {
                let conf = config.lock().await;
                println!("{}", "=== od-cli Configuration ===".bold().cyan());
                println!("Config Path:   {}", Config::config_path()?.display().to_string().dimmed());
                println!("Client ID:     {}", conf.get_client_id().bright_green());
                println!("Tenant ID:     {}", conf.get_tenant_id().bright_green());
                println!(
                    "Chunk Size:    {} MB",
                    conf.chunk_size_mb.unwrap_or(config::DEFAULT_CHUNK_SIZE_MB).to_string().bright_yellow()
                );
                println!("Logged In:     {}", if conf.access_token.is_some() { "Yes".green() } else { "No".red() });
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
            println!("{} Created folder '{}' (ID: {})", "✓".green().bold(), item.name.cyan(), item.id.dimmed());
        }

        Some(Commands::Upload(args)) => {
            let local_p = Path::new(&args.local_path);
            let remote_dest = args.remote_path.as_deref().unwrap_or("");

            if local_p.is_dir() || args.recursive {
                println!("Uploading directory {:?} -> '{}'...", local_p, remote_dest);
                client.upload_directory(local_p, remote_dest).await?;
                println!("{} Directory upload complete!", "✓".green().bold());
            } else {
                let item = client.upload_file(local_p, remote_dest, true).await?;
                println!("{} Uploaded '{}' (Size: {})", "✓".green().bold(), item.name.cyan(), ui::format_size(item.size.unwrap_or(0)));
            }
        }

        Some(Commands::Download(args)) => {
            let local_dst = PathBuf::from(&args.local_path);
            let is_dir = client.get_item(&args.remote_path).await.map(|i| i.is_dir()).unwrap_or(false);

            if is_dir || args.recursive {
                println!("Downloading directory '{}' -> {:?}...", args.remote_path, local_dst);
                client.download_directory(&args.remote_path, &local_dst).await?;
                println!("{} Directory download complete!", "✓".green().bold());
            } else {
                client.download_file(&args.remote_path, &local_dst, true).await?;
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
            println!("{} Moved '{}' -> '{}'", "✓".green().bold(), args.source.dimmed(), item.name.cyan());
        }

        Some(Commands::Cp(args)) => {
            client.copy_item(&args.source, &args.target).await?;
            println!("{} Initiated copy '{}' -> '{}'", "✓".green().bold(), args.source.dimmed(), args.target.cyan());
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
                println!("{} Share link created successfully ({}):", "✓".green().bold(), args.link_type.to_string().cyan());
                println!("   {}", url.bright_blue().underline());
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
                    println!("{}{:<30} {:>10}", indent, item.name, ui::format_size(size).dimmed());
                }
            }
        }
        Ok(())
    })
}
