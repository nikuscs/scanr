use anyhow::Result;

use crate::cli::IndexArgs;

pub async fn run(args: &IndexArgs) -> Result<()> {
    // Clear first, then index with force
    crate::commands::clear::run(&args.root).await?;

    let forced = IndexArgs { force: true, ..args.clone() };
    crate::commands::index::run(&forced).await
}
