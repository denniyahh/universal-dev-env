use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "ude",
    author = "Dennis Kim <d10475u5@outlook.com>",
    version,
    about = "Universal Development Environment & Configuration Engine",
    long_about = "Auditable, iterable, and bidirectionally synchronizable AI development environment system."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Audit machine-global setup and repository baseline for drift
    Doctor(DoctorArgs),
    /// Initialize a repository with the portable project baseline
    Init(InitArgs),
    /// Manage project baselines
    Project(ProjectCommands),
    /// Bidirectional synchronization with the central configuration repository
    Sync(SyncArgs),
}

#[derive(Args, Debug)]
pub struct ProjectCommands {
    #[command(subcommand)]
    pub action: ProjectAction,
}

#[derive(Subcommand, Debug)]
pub enum ProjectAction {
    /// Initialize a repository with the portable project baseline
    Init(InitArgs),
}

#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Audit host machine configuration only
    #[arg(long, alias = "host")]
    pub host_only: bool,

    /// Target repository path to audit (defaults to current directory)
    #[arg(short, long)]
    pub repo: Option<PathBuf>,

    /// Output format (text or json)
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    /// Destination path for new project (positional)
    #[arg(value_name = "PATH")]
    pub positional_path: Option<PathBuf>,

    /// Destination path for new project (flag)
    #[arg(long)]
    pub path: Option<PathBuf>,

    /// Project ecosystem type (rust, node, polyglot)
    #[arg(short = 't', long = "type", default_value = "rust")]
    pub project_type: String,

    /// Enable MemPalace memory stage
    #[arg(long, default_value_t = true)]
    pub mempalace: bool,

    /// Enable Graphify knowledge graph scaffold
    #[arg(long, default_value_t = true)]
    pub graphify: bool,
}

impl InitArgs {
    pub fn resolved_path(&self) -> PathBuf {
        self.path
            .clone()
            .or_else(|| self.positional_path.clone())
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

#[derive(Args, Debug)]
pub struct SyncArgs {
    /// Remote git repository URL to synchronize with (defaults to central_repo in .ude.toml or cached upstream)
    #[arg(long, global = true)]
    pub remote: Option<String>,

    /// Branch to synchronize with (defaults to main)
    #[arg(long, global = true)]
    pub branch: Option<String>,

    #[command(subcommand)]
    pub action: SyncAction,
}

#[derive(Subcommand, Debug)]
pub enum SyncAction {
    /// Show drift status between local files and upstream templates
    Status,
    /// Pull updates from the central store into local environment
    Pull,
    /// Push sanitized local updates back to the central repository
    Push {
        /// Force push ignoring non-critical warnings
        #[arg(long)]
        force: bool,

        /// Open a GitHub Pull Request via gh instead of pushing directly to target branch
        #[arg(long)]
        pr: bool,
    },
}
