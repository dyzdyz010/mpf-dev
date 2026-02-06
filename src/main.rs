mod config;
mod commands;

use clap::{Parser, Subcommand};
use anyhow::Result;

#[derive(Parser)]
#[command(name = "mpf-dev")]
#[command(about = "MPF Development Environment CLI Tool")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download and install MPF SDK
    Setup {
        /// SDK version to install (default: latest)
        #[arg(short, long)]
        version: Option<String>,
    },
    
    /// List installed SDK versions
    Versions,
    
    /// Switch to a specific SDK version
    Use {
        /// Version to use
        version: String,
    },
    
    /// Link a component for source development
    Link {
        #[command(subcommand)]
        action: LinkAction,
    },
    
    /// Unregister a component from source development
    Unlink {
        /// Component name (or "all" to unlink everything)
        component: String,
    },
    
    /// Show current development configuration status
    Status,
    
    /// Print environment variables for manual shell setup
    Env,
    
    /// Run MPF host with development overrides
    Run {
        /// Enable debug mode
        #[arg(short, long)]
        debug: bool,
        
        /// Additional arguments to pass to mpf-host
        #[arg(last = true)]
        args: Vec<String>,
    },
    
    /// Manage full-source workspace (all components from source)
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
}

#[derive(Subcommand)]
enum LinkAction {
    /// Link a plugin build output (auto-derives lib, qml, plugin paths)
    Plugin {
        /// Plugin name (e.g., orders, rules)
        name: String,
        /// Path to plugin build output directory
        path: String,
    },
    
    /// Link the host build output (auto-derives bin, qml paths)
    Host {
        /// Path to host build output directory
        path: String,
    },
    
    /// Link a library component (ui-components, http-client, etc.)
    Component {
        /// Component name (e.g., ui-components, http-client)
        name: String,
        /// Path to component build output directory
        path: String,
    },
    
    /// Link with manual path specification (advanced)
    Manual {
        /// Component name
        name: String,
        /// Path to library directory
        #[arg(long)]
        lib: Option<String>,
        /// Path to QML directory
        #[arg(long)]
        qml: Option<String>,
        /// Path to plugin directory
        #[arg(long)]
        plugin: Option<String>,
        /// Path to headers/include directory
        #[arg(long)]
        headers: Option<String>,
        /// Path to bin directory
        #[arg(long)]
        bin: Option<String>,
    },
}

#[derive(Subcommand)]
enum WorkspaceAction {
    /// Initialize a new workspace with all MPF components
    Init {
        /// Workspace directory (default: current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
    
    /// Build all components in workspace
    Build {
        /// Build type: Debug or Release
        #[arg(short, long, default_value = "Debug")]
        config: String,
    },
    
    /// Run mpf-host from workspace
    Run {
        /// Additional arguments to pass to mpf-host
        #[arg(last = true)]
        args: Vec<String>,
    },
    
    /// Show workspace status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Setup { version } => commands::setup(version).await,
        Commands::Versions => commands::versions(),
        Commands::Use { version } => commands::use_version(&version),
        Commands::Link { action } => commands::link_action(action),
        Commands::Unlink { component } => commands::unlink(&component),
        Commands::Status => commands::status(),
        Commands::Env => commands::env_vars(),
        Commands::Run { debug, args } => commands::run(debug, args),
        Commands::Workspace { action } => match action {
            WorkspaceAction::Init { path } => commands::workspace_init(path),
            WorkspaceAction::Build { config } => commands::workspace_build(&config),
            WorkspaceAction::Run { args } => commands::workspace_run(args),
            WorkspaceAction::Status => commands::workspace_status(),
        },
    }
}

// Re-export LinkAction for use in commands module
pub use crate::LinkAction;
