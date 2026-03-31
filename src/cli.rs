use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "scanr",
    version,
    about = "Fast semantic codebase search via OpenAI embeddings + pgvector"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create database and schema (run once per machine)
    Setup(SetupArgs),

    /// Index or re-index the project (incremental)
    Index(IndexArgs),

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

        /// Minimum similarity score (0-1)
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

    /// Compact project structure overview for fast orientation
    Tree {
        /// Project root directory
        #[arg(long, default_value = ".")]
        root: String,

        /// Focus on a subdirectory within the project root
        #[arg(long)]
        path: Option<String>,

        /// Max branching depth before collapsing subtrees
        #[arg(long, default_value_t = 6)]
        depth: usize,

        /// Max files shown per line before wrapping
        #[arg(long, default_value_t = 6)]
        inline: usize,

        /// Include test directories and test files
        #[arg(long)]
        all: bool,
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

    /// Clear and re-index the project from scratch
    Reindex(IndexArgs),

    /// Structural scan: extract functions, bindings, and exports from TypeScript/JavaScript files
    Scan(ScanArgs),
}

#[derive(clap::Args, Clone)]
pub struct ScanArgs {
    /// Project root directory
    #[arg(long, default_value = ".")]
    pub root: String,

    /// Output format
    #[arg(long, default_value = "compact")]
    pub mode: crate::scan::types::OutputMode,

    /// File extensions to include (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub include: Vec<String>,

    /// Patterns to exclude (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub exclude: Vec<String>,

    /// Max file size in bytes
    #[arg(long, default_value_t = 1_048_576)]
    pub max_bytes: u64,

    /// Function kinds to include
    #[arg(long, default_value = "all")]
    pub function_kinds: crate::scan::types::FunctionKindsFilter,

    /// Scan a single file instead of directory
    #[arg(long)]
    pub file: Option<String>,
}

#[derive(clap::Args, Clone)]
pub struct SetupArgs {
    /// `PostgreSQL` version to install (e.g. 17, 18)
    #[arg(long, default_value_t = 18)]
    pub pg_version: u32,

    /// Skip prompts, accept defaults (for non-interactive / agent use)
    #[arg(short, long)]
    pub yes: bool,
}

#[derive(clap::Args, Clone)]
pub struct IndexArgs {
    /// Project root directory
    #[arg(long, default_value = ".")]
    pub root: String,

    /// Re-index a single file
    #[arg(long)]
    pub file: Option<String>,

    /// Path to a custom gitignore file
    #[arg(long)]
    pub gitignore: Option<String>,

    /// Force re-embed everything
    #[arg(long)]
    pub force: bool,

    /// Chunk size in characters
    #[arg(long, default_value_t = 1000)]
    pub chunk_size: usize,

    /// Chunk overlap in characters
    #[arg(long, default_value_t = 100)]
    pub chunk_overlap: usize,

    /// Skip files larger than this (bytes). Default: 512 KB.
    #[arg(long, default_value_t = 524_288)]
    pub max_bytes: u64,
}
