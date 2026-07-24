mod as_is;
mod granola;

pub use as_is::AsIsData;

use crate::types::{Collection, ImportSource, ImportSourceInfo, ImportStats, TransformKind};

pub async fn import_all(source: &ImportSource) -> Result<Collection, crate::Error> {
    match source.transform {
        TransformKind::Granola => granola::import_all_from_path(&source.path).await,
        TransformKind::AsIs => as_is::load_data(&source.path),
    }
}

pub async fn import_stats(source: &ImportSource) -> Result<ImportStats, crate::Error> {
    let data = import_all(source).await?;
    Ok(ImportStats::from_data(&data))
}

pub fn all_sources() -> Vec<ImportSource> {
    [ImportSource::granola()].into_iter().flatten().collect()
}

pub fn list_available_sources() -> Vec<ImportSourceInfo> {
    all_sources()
        .into_iter()
        .filter(|s| s.is_available())
        .map(|s| s.info())
        .collect()
}
