pub mod extract;
pub mod parse;

// Re-export for convenience: `scan::typescript::process_file`
#[allow(unused_imports)]
pub use parse::process_file;
