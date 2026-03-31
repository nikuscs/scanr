use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod chunk;
mod cli;
mod commands;
mod db;
mod embed;
mod git;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Setup => commands::setup::run().await,
        Commands::Index(args) => commands::index::run(&args).await,
        Commands::Search { query, root, limit, threshold, lang, files_only, json } => {
            let q = query.join(" ");
            commands::search::run(&q, &root, limit, threshold, lang, files_only, json).await
        }
        Commands::Status { root } => commands::status::run(&root).await,
        Commands::Clear { root } => commands::clear::run(&root).await,
        Commands::Reindex(args) => commands::reindex::run(&args).await,
    }
}
