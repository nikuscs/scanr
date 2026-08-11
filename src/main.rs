use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod scan;

use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Tree { root, path, depth, inline, all } => {
            commands::tree::run(&root, path.as_deref(), depth, inline, all)
        }
        Commands::Scan(args) => commands::scan::run(&args),
    }
}
