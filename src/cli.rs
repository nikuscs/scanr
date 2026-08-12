use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "scanr",
    version,
    about = "Static analysis and search for TypeScript and JavaScript"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Find duplicate and similar functions and types
    Dupes(DupesArgs),

    /// Report deterministic, evidence-backed code-slop review signals
    Slop(SlopArgs),

    /// Search file contents or paths
    Search(SearchArgs),

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

        /// Annotate files with compact function trees and analysis markers
        #[arg(long)]
        functions: bool,

        /// Show anonymous callbacks instead of summarizing their count
        #[arg(long, requires = "functions")]
        all_functions: bool,

        /// Maximum lines for low-value function candidates
        #[arg(long, default_value_t = 3, requires = "functions")]
        low_value_max_lines: u32,

        /// Similarity threshold for duplicate-group markers (0-1)
        #[arg(long, default_value_t = 0.87, requires = "functions")]
        duplicate_threshold: f64,

        /// Render nested functions with plain-English analysis details
        #[arg(long, requires = "functions")]
        function_details: bool,

        /// Minimum function length shown by --function-details
        #[arg(long, default_value_t = 3, requires = "function_details")]
        function_min_lines: u32,

        /// Maximum function length shown by --function-details
        #[arg(long, default_value_t = 10, requires = "function_details")]
        function_max_lines: u32,

        /// Show deterministic code-health metrics and findings
        #[arg(long, conflicts_with = "function_details")]
        health: bool,

        /// Show only files with health findings
        #[arg(long, requires = "health")]
        only_findings: bool,

        /// Limit health output to the first N files after sorting
        #[arg(long, requires = "health")]
        top: Option<usize>,

        /// Rank health output by a specific metric
        #[arg(long, value_enum, requires = "health")]
        sort_by: Option<HealthSort>,

        /// Emit structured JSON for --health
        #[arg(long, requires = "health")]
        json: bool,
    },

    /// Structural scan: extract functions, bindings, and exports from TypeScript/JavaScript files
    Scan(ScanArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum HealthSort {
    Severity,
    Coupling,
    Duplicates,
    Size,
}

#[derive(clap::Args, Clone)]
pub struct DupesArgs {
    /// Project root directory
    #[arg(long, default_value = ".")]
    pub root: String,

    /// Minimum similarity score (0-1)
    #[arg(long, default_value_t = 0.87)]
    pub threshold: f64,

    /// Minimum function length in lines
    #[arg(long, default_value_t = 3)]
    pub min_lines: u32,

    /// Include similar type and type-literal pairs
    #[arg(long)]
    pub types: bool,

    /// Include source text for each match
    #[arg(long)]
    pub print: bool,
}

#[derive(clap::Args, Clone)]
pub struct SlopArgs {
    /// Project root directory
    #[arg(long, default_value = ".")]
    pub root: String,

    /// Git base ref that enables the three diff-only finding kinds
    #[arg(long)]
    pub base: Option<String>,

    /// Minimum confidence to include
    #[arg(long, value_enum, default_value_t = crate::slop::types::SlopConfidence::Medium)]
    pub confidence: crate::slop::types::SlopConfidence,

    /// Include only these finding kinds (comma-separated, repeatable)
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,

    /// Exclude these finding kinds (comma-separated, repeatable)
    #[arg(long, value_delimiter = ',')]
    pub exclude: Vec<String>,

    /// Limit findings after deterministic sorting
    #[arg(long)]
    pub top: Option<usize>,

    /// Include test files in ordinary detector analysis
    #[arg(long)]
    pub include_test_files: bool,

    /// Emit stable schema-versioned JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args, Clone)]
pub struct SearchArgs {
    /// Literal content pattern (omit when using --path)
    #[arg(required_unless_present = "path")]
    pub pattern: Option<String>,

    /// Search file paths with this substring or glob instead of file contents
    #[arg(long, value_name = "PATTERN", conflicts_with = "pattern")]
    pub path: Option<String>,

    /// Project root directory
    #[arg(long, default_value = ".")]
    pub root: String,

    /// Match ASCII letters case-insensitively
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// Include only paths matching this gitignore-style glob (repeatable)
    #[arg(long)]
    pub glob: Vec<String>,

    /// Stop after this many matching lines per file
    #[arg(long)]
    pub max_count: Option<usize>,

    /// Include this many lines before and after each content match
    #[arg(long, default_value_t = 0)]
    pub context: usize,

    /// Return structured JSON output
    #[arg(long)]
    pub json: bool,
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

    /// Rules to run (comma-separated; default: all)
    #[arg(long, value_delimiter = ',')]
    pub rules: Vec<String>,

    /// Include test files in scope and low-value checks
    #[arg(long)]
    pub include_test_files: bool,

    /// Maximum lines for low-value function candidates
    #[arg(long, default_value_t = 3)]
    pub low_value_max_lines: u32,

    /// Include ordinary small bodies and component-local TSX functions
    #[arg(long)]
    pub loose_low_value: bool,

    /// Minimum lines for a dominant function or class
    #[arg(long, default_value_t = 300)]
    pub dominant_container_min_lines: u32,

    /// Minimum tiny helpers beside a dominant function or class
    #[arg(long, default_value_t = 2)]
    pub dominant_helper_min_count: usize,

    /// Scan a single file instead of directory
    #[arg(long)]
    pub file: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_slop_command_contract() {
        let cli = Cli::try_parse_from([
            "scanr",
            "slop",
            "--root",
            "project",
            "--confidence",
            "high",
            "--only",
            "suppression-chain,patch-stack",
            "--exclude",
            "dead-surface",
            "--top",
            "0",
            "--include-test-files",
            "--json",
        ])
        .unwrap();
        let Commands::Slop(args) = cli.command else {
            panic!("expected slop command");
        };
        assert_eq!(args.root, "project");
        assert_eq!(args.confidence, crate::slop::types::SlopConfidence::High);
        assert_eq!(args.only, ["suppression-chain", "patch-stack"]);
        assert_eq!(args.exclude, ["dead-surface"]);
        assert_eq!(args.top, Some(0));
        assert!(args.include_test_files);
        assert!(args.json);
    }
}
