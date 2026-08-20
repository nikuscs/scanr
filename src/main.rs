use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod scan;
mod similarity;
mod slop;

use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Dupes(args) => commands::dupes::run(&args),
        Commands::Slop(args) => commands::slop::run(&args),
        Commands::Refs(args) => commands::refs::run(&args),
        Commands::Search(args) => commands::search::run(&args),
        Commands::Tree {
            root,
            path,
            depth,
            inline,
            all,
            functions,
            all_functions,
            low_value_max_lines,
            duplicate_threshold,
            function_details,
            function_min_lines,
            function_max_lines,
            health,
            only_findings,
            top,
            sort_by,
            json,
        } => commands::tree::run(
            &root,
            path.as_deref(),
            depth,
            inline,
            all,
            functions,
            all_functions,
            low_value_max_lines,
            duplicate_threshold,
            function_details,
            function_min_lines,
            function_max_lines,
            health,
            only_findings,
            top,
            sort_by,
            json,
        ),
        Commands::Scan(args) => commands::scan::run(&args),
    }
}
