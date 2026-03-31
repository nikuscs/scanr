use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "scanr", about = "Fast semantic codebase search via OpenAI embeddings + pgvector")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create database and schema (run once per machine)
    Setup,

    /// Index or re-index the project (incremental)
    Index {
        /// Project root directory
        #[arg(long, default_value = ".")]
        root: String,

        /// Re-index a single file
        #[arg(long)]
        file: Option<String>,

        /// Force re-embed everything
        #[arg(long)]
        force: bool,
    },

    /// Semantic search across the indexed codebase
    Search {
        /// Search query
        query: Vec<String>,

        /// Project root directory
        #[arg(long, default_value = ".")]
        root: String,

        /// Number of results
        #[arg(long, default_value_t = 10)]
        limit: usize,

        /// Minimum similarity score (0–1)
        #[arg(long, default_value_t = 0.0)]
        threshold: f64,

        /// Filter by language extension (e.g. ts, tsx, md)
        #[arg(long)]
        lang: Option<String>,

        /// Return file paths only (no snippets)
        #[arg(long)]
        files_only: bool,

        /// Return structured JSON output
        #[arg(long)]
        json: bool,
    },

    /// Show indexing stats and stale file count
    Status {
        /// Project root directory
        #[arg(long, default_value = ".")]
        root: String,
    },

    /// Remove all indexed data for a project
    Clear {
        /// Project root directory
        #[arg(long, default_value = ".")]
        root: String,
    },
}
