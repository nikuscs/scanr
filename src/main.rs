use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod dependencies;
mod git;
mod rename;
mod scan;
mod similarity;
mod slop;
mod worker;

use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Deps(args) => commands::deps::run(&args),
        Commands::Diff(args) => commands::diff::run(&args),
        Commands::Dupes(args) => commands::dupes::run(&args),
        Commands::Slop(args) => commands::slop::run(&args),
        Commands::Refs(args) => commands::refs::run(&args),
        Commands::Roles(args) => commands::roles::run(&args),
        Commands::Rename(args) => commands::rename::run(&args),
        Commands::Search(args) => commands::search::run(&args),
        Commands::Tree(args) => commands::tree::run(&args),
        Commands::Scan(args) => commands::scan::run(&args),
        Commands::Xray(args) => commands::xray::run(&args),
    }
}
