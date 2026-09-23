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
    /// Bidirectional synchronization with the central configuration repository
    Sync(SyncArgs),
}

#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Audit host machine configuration only
    #[arg(long)]
    pub host_only: bool,

    /// Target repository path to audit (defaults to current directory)
    #[arg(short, long)]
    pub repo: Option<PathBuf>,

    /// Output format (text or json)
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Destination path for new project (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Project ecosystem type (rust, node, polyglot)
    #[arg(short, long, default_value = "rust")]
    pub project_type: String,

    /// Enable MemPalace memory stage
    #[arg(long, default_value_t = true)]
    pub mempalace: bool,

    /// Enable Graphify knowledge graph scaffold
    #[arg(long, default_value_t = true)]
    pub graphify: bool,
}

#[derive(Args, Debug)]
pub struct SyncArgs {
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
    },
}
