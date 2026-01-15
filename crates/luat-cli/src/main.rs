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
    }
}
