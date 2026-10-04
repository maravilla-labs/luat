// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use clap::{Parser, Subcommand};
use luat_cli::commands;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "luat")]
#[command(author = "Maravilla Labs")]
#[command(version)]
#[command(about = "Svelte-inspired server-side Lua templating CLI", long_about = None)]
struct Cli {
    /// Log level: error, warn, info, debug, trace
    #[arg(long, global = true, default_value = "warn")]
    log_level: String,

    /// Verbose mode: show all tool output without filtering
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Quiet mode: only show errors (useful for CI)
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new luat project
    Init {
        /// Project name (defaults to current directory name)
        name: Option<String>,
        /// Template to use: default, htmx
        #[arg(short, long, default_value = "default")]
        template: String,
    },
    /// Start development server with live reload
    Dev {
        /// Port to run the dev server on
        #[arg(short, long, default_value = "3000")]
        port: u16,
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    /// Build templates for production
    Build {
        /// Output Lua source instead of binary
        #[arg(long)]
        source: bool,
        /// Output directory
        #[arg(short, long, default_value = "dist")]
        output: String,
    },
    /// Serve production build (no live reload, optimized)
    Serve {
        /// Port to run the server on
        #[arg(short, long, default_value = "3000")]
        port: u16,
        /// Host to bind to
        #[arg(long, default_value = "0.0.0.0")]
        host: String,
    },
    /// Watch files and rebuild on change (no server)
    Watch,
    /// Add a package dependency: @scope/name[@<requirement>]
    Add {
        /// Package, e.g. @acme/ui or @acme/ui@^1.2
        package: String,
        /// Use a local directory instead of a registry (path dependency)
        #[arg(long)]
        path: Option<String>,
    },
    /// Remove a package dependency
    Remove {
        /// Package name, e.g. @acme/ui
        package: String,
    },
    /// Install the packages pinned in luat.lock (resolving first when needed)
    Install {
        /// Fail instead of resolving when luat.lock is missing or out of date
        #[arg(long)]
        frozen: bool,
    },
    /// Update dependencies to the newest matching versions
    Update {
        /// Only update this package
        package: Option<String>,
    },
    /// Search a registry for packages
    Search {
        /// Search text
        query: String,
        /// Registry URL (default: the project's default registry)
        #[arg(long)]
        registry: Option<String>,
    },
    /// Write the package tarball without publishing it
    Pack,
    /// Publish the package in the current directory
    Publish {
        /// Pack and check, but do not upload
        #[arg(long)]
        dry_run: bool,
    },
    /// Store a registry token in the user's config directory
    Login {
        /// Registry URL (default: the project's default registry)
        registry: Option<String>,
        /// The token (read from stdin when omitted)
        #[arg(long)]
        token: Option<String>,
    },
    /// Yank a published version: @scope/name@<version>
    Yank {
        /// Package and version, e.g. @acme/ui@1.2.0
        package: String,
        /// Unyank instead
        #[arg(long)]
        undo: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing with the specified log level
    let filter = EnvFilter::try_new(&cli.log_level)
        .unwrap_or_else(|_| EnvFilter::new("warn"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    match cli.command {
        Commands::Init { name, template } => {
            commands::init::run(name, Some(template)).await
        }
        Commands::Dev { port, host } => {
            commands::dev::run(&host, port, cli.verbose, cli.quiet).await
        }
        Commands::Build { source, output } => {
            commands::build::run(source, &output).await
        }
        Commands::Serve { port, host } => {
            commands::serve::run(&host, port).await
        }
        Commands::Watch => {
            commands::watch::run().await
        }
        Commands::Add { package, path } => commands::packages::add(&package, path.as_deref()).await,
        Commands::Remove { package } => commands::packages::remove(&package).await,
        Commands::Install { frozen } => commands::packages::install(frozen).await,
        Commands::Update { package } => commands::packages::update(package.as_deref()).await,
        Commands::Search { query, registry } => {
            commands::packages::registry::search(&query, registry.as_deref()).await
        }
        Commands::Pack => commands::packages::registry::pack(),
        Commands::Publish { dry_run } => commands::packages::registry::publish(dry_run).await,
        Commands::Login { registry, token } => {
            commands::packages::registry::login(registry.as_deref(), token).await
        }
        Commands::Yank { package, undo } => commands::packages::registry::yank(&package, undo).await,
    }
}
