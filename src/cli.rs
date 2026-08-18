use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "od-cli",
    author = "OneDrive CLI Contributors",
    version = "0.1.0",
    about = "A fast and modern CLI and interactive shell for Microsoft OneDrive using Azure App",
    long_about = "od-cli is a powerful command-line interface for Microsoft OneDrive.\nRun commands directly or start an interactive shell by running `od-cli` without arguments."
)]
pub struct Cli {
    /// Force IPv4 network resolution
    #[arg(short = '4', long = "ipv4", global = true)]
    pub ipv4: bool,

    /// Force IPv6 network resolution
    #[arg(short = '6', long = "ipv6", global = true)]
    pub ipv6: bool,

    /// Number of concurrent worker threads for upload/download
    #[arg(short = 'j', long = "threads", global = true)]
    pub threads: Option<usize>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Authenticate with Microsoft Azure AD (OAuth2 Device Code Flow)
    Auth(AuthArgs),

    /// View or update CLI configuration (client_id, tenant_id, chunk_size, etc.)
    Config(ConfigArgs),

    /// List files and folders in OneDrive
    #[command(alias = "list")]
    Ls(LsArgs),

    /// Show detailed metadata of a file/folder or drive storage quota
    #[command(alias = "stat")]
    Info(InfoArgs),

    /// Show OneDrive storage usage and quota
    Quota,

    /// Create a new folder
    Mkdir(MkdirArgs),

    /// Upload a local file or directory to OneDrive
    #[command(alias = "put")]
    Upload(UploadArgs),

    /// Download a remote file or directory from OneDrive
    #[command(alias = "get")]
    Download(DownloadArgs),

    /// Print remote file contents to stdout
    Cat(CatArgs),

    /// Delete a remote file or folder
    #[command(alias = "delete")]
    Rm(RmArgs),

    /// Move or rename a remote file or folder
    #[command(alias = "move")]
    Mv(MvArgs),

    /// Copy a remote file or folder asynchronously
    #[command(alias = "copy")]
    Cp(CpArgs),

    /// Search for files and folders across OneDrive
    Search(SearchArgs),

    /// Create a shareable link for a file or folder
    Share(ShareArgs),

    /// Launch the interactive REPL shell
    #[command(alias = "interactive", alias = "repl")]
    Shell,
}

#[derive(Args, Debug, Clone)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub action: AuthAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum AuthAction {
    /// Initiate Azure Device Code login flow
    Login,
    /// Log out and clear saved tokens
    Logout,
    /// Check current authentication status
    #[command(alias = "whoami")]
    Status,
}

#[derive(Args, Debug, Clone)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigAction {
    /// Set a config key (client_id, tenant_id, chunk_size_mb)
    Set {
        /// Configuration key (client_id, tenant_id, chunk_size_mb)
        key: String,
        /// Value to set
        value: String,
    },
    /// Get the value of a config key
    Get {
        /// Configuration key
        key: String,
    },
    /// Show all configuration settings
    #[command(alias = "list")]
    Show,
    /// Print the path of the config file
    Path,
}

#[derive(Args, Debug, Clone)]
pub struct LsArgs {
    /// Remote directory path (default: root "/")
    #[arg(default_value = "/")]
    pub path: String,

    /// Show detailed long listing with IDs
    #[arg(short = 'l', long)]
    pub long: bool,

    /// Recursively list contents of all subdirectories
    #[arg(short = 'r', long)]
    pub recursive: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct InfoArgs {
    /// Remote file or folder path (if omitted or "/", shows drive quota)
    #[arg(default_value = "/")]
    pub path: String,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct MkdirArgs {
    /// Remote folder path to create
    pub path: String,

    /// Create intermediate parent directories as needed
    #[arg(short = 'p', long)]
    pub parents: bool,
}

#[derive(Args, Debug, Clone)]
pub struct UploadArgs {
    /// Local file or directory path
    pub local_path: String,

    /// Remote destination path on OneDrive
    pub remote_path: Option<String>,

    /// Recursively upload directory
    #[arg(short = 'r', long)]
    pub recursive: bool,

    /// Concurrency worker threads for upload
    #[arg(short = 'j', long = "threads")]
    pub threads: Option<usize>,
}

#[derive(Args, Debug, Clone)]
pub struct DownloadArgs {
    /// Remote file or directory path on OneDrive
    pub remote_path: String,

    /// Local destination path (default: current directory or same filename)
    #[arg(default_value = ".")]
    pub local_path: String,

    /// Recursively download directory
    #[arg(short = 'r', long)]
    pub recursive: bool,

    /// Concurrency worker threads for download
    #[arg(short = 'j', long = "threads")]
    pub threads: Option<usize>,
}

#[derive(Args, Debug, Clone)]
pub struct CatArgs {
    /// Remote file path
    pub path: String,
}

#[derive(Args, Debug, Clone)]
pub struct RmArgs {
    /// Remote file or folder path to delete
    pub path: String,
}

#[derive(Args, Debug, Clone)]
pub struct MvArgs {
    /// Source remote file or folder path
    pub source: String,

    /// Target remote path or new name
    pub target: String,
}

#[derive(Args, Debug, Clone)]
pub struct CpArgs {
    /// Source remote file or folder path
    pub source: String,

    /// Target destination directory or path
    pub target: String,
}

#[derive(Args, Debug, Clone)]
pub struct SearchArgs {
    /// Search keyword query
    pub query: String,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ShareArgs {
    /// Remote file or folder path
    pub path: String,

    /// Type of sharing link
    #[arg(short = 't', long, value_enum, default_value_t = ShareType::View)]
    pub link_type: ShareType,

    /// Scope of sharing link
    #[arg(short = 's', long, value_enum, default_value_t = ShareScope::Anonymous)]
    pub scope: ShareScope,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareType {
    View,
    Edit,
    Embed,
}

impl std::fmt::Display for ShareType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShareType::View => write!(f, "view"),
            ShareType::Edit => write!(f, "edit"),
            ShareType::Embed => write!(f, "embed"),
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareScope {
    Anonymous,
    Organization,
}

impl std::fmt::Display for ShareScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShareScope::Anonymous => write!(f, "anonymous"),
            ShareScope::Organization => write!(f, "organization"),
        }
    }
}
