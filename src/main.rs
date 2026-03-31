use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod cli;
mod commands;
mod index;
mod scan;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Setup(args) => commands::setup::run(&args).await,
        Commands::Index(args) => commands::index::run(&args).await,
        Commands::Search { query, root, limit, threshold, lang, files_only, json } => {
            let q = query.join(" ");
            commands::search::run(&q, &root, limit, threshold, lang, files_only, json).await
        }
        Commands::Tree { root, path, depth, inline, all } => {
            commands::tree::run(&root, path.as_deref(), depth, inline, all).await
        }
        Commands::Status { root } => commands::status::run(&root).await,
        Commands::Clear { root } => commands::clear::run(&root).await,
        Commands::Reindex(args) => commands::reindex::run(&args).await,
        Commands::List => commands::list::run().await,
        Commands::Scan(args) => commands::scan::run(&args).await,
    }
}
