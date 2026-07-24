use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use hypr_importer_core::ir::{
    Collection, CollectionStats, EnhancedNote, Human, Organization, Session, SessionParticipant,
    Tag, TagMapping, Template, TemplateSection, Transcript, Word,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TransformKind {
    Granola,
    AsIs,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ImportSourceKind {
    Granola,
    AsIs,
}

#[derive(Debug, Clone)]
pub struct ImportSource {
    pub kind: Option<ImportSourceKind>,
    pub transform: TransformKind,
    pub path: PathBuf,
    pub name: String,
}

impl ImportSource {
    pub fn from_path(path: PathBuf, transform: TransformKind) -> Self {
        Self {
            kind: None,
            transform,
            path: path.clone(),
            name: path.to_string_lossy().to_string(),
        }
    }

    pub fn granola() -> Option<Self> {
        let path = hypr_granola::default_supabase_path();
        Some(Self {
            kind: Some(ImportSourceKind::Granola),
            transform: TransformKind::Granola,
            path,
            name: "Granola".to_string(),
        })
    }

    pub fn is_available(&self) -> bool {
        self.path.exists()
    }

    pub fn info(&self) -> ImportSourceInfo {
        let path_str = self.path.to_string_lossy().to_string();
        let (display_path, reveal_path) = (path_str.clone(), path_str);

        ImportSourceInfo {
            kind: self.kind.clone(),
            transform: self.transform,
            name: self.name.clone(),
            path: display_path,
            reveal_path,
        }
    }
}

impl From<ImportSourceKind> for ImportSource {
    fn from(kind: ImportSourceKind) -> Self {
        match kind {
            ImportSourceKind::Granola => Self::granola().unwrap(),
            ImportSourceKind::AsIs => Self {
                kind: Some(ImportSourceKind::AsIs),
                transform: TransformKind::AsIs,
                path: PathBuf::new(),
                name: "JSON Import".to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportSourceInfo {
    pub kind: Option<ImportSourceKind>,
    pub transform: TransformKind,
    pub name: String,
    pub path: String,
    pub reveal_path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportStats {
    pub sessions_count: usize,
    pub transcripts_count: usize,
    pub humans_count: usize,
    pub organizations_count: usize,
    pub participants_count: usize,
    pub templates_count: usize,
    pub enhanced_notes_count: usize,
}

impl ImportStats {
    pub fn from_data(data: &Collection) -> Self {
        CollectionStats::from_collection(data).into()
    }
}

impl From<CollectionStats> for ImportStats {
    fn from(stats: CollectionStats) -> Self {
        Self {
            sessions_count: stats.sessions_count,
            transcripts_count: stats.transcripts_count,
            humans_count: stats.humans_count,
            organizations_count: stats.organizations_count,
            participants_count: stats.participants_count,
            templates_count: stats.templates_count,
            enhanced_notes_count: stats.enhanced_notes_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportDataResult {
    pub stats: ImportStats,
    pub data: serde_json::Value,
}
