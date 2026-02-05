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
    
    /// Register a component for source development
    Link {
        /// Component name (e.g., http-client, ui-components, plugin-orders, host)
        component: String,
        
        /// Path to built library directory
        #[arg(long)]
        lib: Option<String>,
        
        /// Path to QML modules directory
        #[arg(long)]
        qml: Option<String>,
        
        /// Path to plugin directory (for plugins)
        #[arg(long)]
        plugin: Option<String>,
        
        /// Path to include directory (for headers)
        #[arg(long, alias = "include")]
        headers: Option<String>,
        
        /// Path to executable binary directory (for host component)
        #[arg(long)]
        bin: Option<String>,
    },
    
    /// Unregister a component from source development
    Unlink {
        /// Component name
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Setup { version } => commands::setup(version).await,
        Commands::Versions => commands::versions(),
        Commands::Use { version } => commands::use_version(&version),
        Commands::Link { component, lib, qml, plugin, headers, bin } => {
            commands::link(&component, lib, qml, plugin, headers, bin)
        }
        Commands::Unlink { component } => commands::unlink(&component),
        Commands::Status => commands::status(),
        Commands::Env => commands::env_vars(),
        Commands::Run { debug, args } => commands::run(debug, args),
    }
}
