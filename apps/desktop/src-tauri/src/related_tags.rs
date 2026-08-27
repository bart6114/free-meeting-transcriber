use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tauri::Manager;
use tauri_plugin_settings::SettingsPluginExt;
use tauri_plugin_tantivy::TantivyPluginExt;

use crate::session_store::{SessionStore, TagSuggestionItem, TagSuggestionStatus};

pub const ALGORITHM_VERSION: u32 = 2;
const CANDIDATE_LIMIT: usize = 50;
const SUGGESTION_LIMIT: usize = 3;
const SUGGESTION_THRESHOLD: f32 = 0.35;
const AUTO_ACCEPT_THRESHOLD: f32 = 0.75;
const RETRY_DELAY: Duration = Duration::from_secs(5);
const NOTE_IDLE_DELAY: Duration = Duration::from_secs(10);
const NOTE_MAX_WAIT: Duration = Duration::from_secs(120);
const TRANSCRIPT_WEIGHT: f32 = 0.65;
const SUMMARY_WEIGHT: f32 = 0.25;
const NOTE_WEIGHT: f32 = 0.10;

enum QueueMessage {
    Process(String),
    DebounceElapsed { session_id: String, generation: u64 },
}

struct DebounceEntry {
    generation: u64,
    first_change: Instant,
}

#[derive(Default)]
struct DebounceState {
    next_generation: u64,
    entries: HashMap<String, DebounceEntry>,
    revisions: HashMap<String, u64>,
}

impl DebounceState {
    fn bump_revision(&mut self, session_id: &str) -> u64 {
        let revision = self
            .revisions
            .get(session_id)
            .copied()
            .unwrap_or_default()
            .wrapping_add(1);
        self.revisions.insert(session_id.to_string(), revision);
        revision
    }

    fn schedule(
        &mut self,
        session_id: &str,
        now: Instant,
        idle_delay: Duration,
        max_wait: Duration,
    ) -> (u64, Duration) {
        self.bump_revision(session_id);
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        let first_change = self
            .entries
            .get(session_id)
            .map(|entry| entry.first_change)
            .unwrap_or(now);
        self.entries.insert(
            session_id.to_string(),
            DebounceEntry {
                generation,
                first_change,
            },
        );
        let deadline = (now + idle_delay).min(first_change + max_wait);
        (generation, deadline.saturating_duration_since(now))
    }

    fn take_if_current(&mut self, session_id: &str, generation: u64) -> bool {
        if self
            .entries
            .get(session_id)
            .is_some_and(|entry| entry.generation == generation)
        {
            self.entries.remove(session_id);
            true
        } else {
            false
        }
    }

    fn revision(&self, session_id: &str) -> u64 {
        self.revisions.get(session_id).copied().unwrap_or_default()
    }
}

#[derive(Clone)]
pub struct RelatedTagQueue {
    sender: tokio::sync::mpsc::UnboundedSender<QueueMessage>,
    debounce: Arc<Mutex<DebounceState>>,
}

impl RelatedTagQueue {
    pub fn enqueue(&self, session_id: String) {
        let mut state = self.debounce.lock().unwrap();
        state.bump_revision(&session_id);
        state.entries.remove(&session_id);
        drop(state);
        let _ = self.sender.send(QueueMessage::Process(session_id));
    }

    pub fn note_changed(&self, session_id: String) {
        let (generation, delay) = self.debounce.lock().unwrap().schedule(
            &session_id,
            Instant::now(),
            NOTE_IDLE_DELAY,
            NOTE_MAX_WAIT,
        );
        let sender = self.sender.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = sender.send(QueueMessage::DebounceElapsed {
                session_id,
                generation,
            });
        });
    }

    fn revision(&self, session_id: &str) -> u64 {
        self.debounce.lock().unwrap().revision(session_id)
    }

    #[cfg(test)]
    pub(crate) fn new_test() -> Self {
        let (sender, _) = tokio::sync::mpsc::unbounded_channel();
        Self {
            sender,
            debounce: Arc::new(Mutex::new(DebounceState::default())),
        }
    }

    #[cfg(test)]
    pub(crate) fn has_debounced_change(&self, session_id: &str) -> bool {
        self.debounce
            .lock()
            .unwrap()
            .entries
            .contains_key(session_id)
    }
}

pub fn spawn<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    store: Arc<SessionStore>,
) -> RelatedTagQueue {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<QueueMessage>();
    let queue = RelatedTagQueue {
        sender,
        debounce: Arc::new(Mutex::new(DebounceState::default())),
    };
    let worker_queue = queue.clone();

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        for entry in store.session_list() {
            if entry
                .meta
                .tag_suggestions
                .as_ref()
                .is_some_and(|state| state.status == TagSuggestionStatus::Pending)
            {
                worker_queue.enqueue(entry.meta.id);
            }
        }

        while let Some(message) = receiver.recv().await {
            let session_id = match message {
                QueueMessage::Process(session_id) => session_id,
                QueueMessage::DebounceElapsed {
                    session_id,
                    generation,
                } => {
                    if !worker_queue
                        .debounce
                        .lock()
                        .unwrap()
                        .take_if_current(&session_id, generation)
                    {
                        continue;
                    }
                    session_id
                }
            };
            if let Err(error) = process(&app, &store, &worker_queue, &session_id).await {
                tracing::warn!(%session_id, %error, "related tags: analysis failed; retrying");
                let retry_queue = worker_queue.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(RETRY_DELAY).await;
                    let _ = retry_queue.sender.send(QueueMessage::Process(session_id));
                });
            }
        }
    });

    queue
}

#[tauri::command]
#[specta::specta]
pub async fn session_queue_tag_suggestions<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    session_id: String,
) -> Result<(), String> {
    if app
        .try_state::<Arc<SessionStore>>()
        .and_then(|store| store.session_get(&session_id))
        .is_none()
    {
        return Err(format!("session {session_id} does not exist"));
    }
    app.state::<RelatedTagQueue>().enqueue(session_id);
    Ok(())
}

async fn process<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SessionStore,
    queue: &RelatedTagQueue,
    session_id: &str,
) -> Result<(), String> {
    let analysis_revision = queue.revision(session_id);
    let source = source_content(store, session_id).await?;
    if !store
        .mark_tag_suggestions_pending(session_id, source.hash.clone(), ALGORITHM_VERSION)
        .await
        .map_err(|error| error.to_string())?
    {
        return Ok(());
    }
    let current = store
        .session_get(session_id)
        .ok_or_else(|| "session disappeared before analysis".to_string())?;
    if term_frequencies(&source.combined).len() < 10 {
        let latest_source = source_content(store, session_id).await?;
        if queue.revision(session_id) != analysis_revision || latest_source.hash != source.hash {
            tracing::debug!(%session_id, "related tags: discarded stale analysis");
            return Ok(());
        }
        store
            .complete_tag_suggestions(
                session_id,
                &source.hash,
                ALGORITHM_VERSION,
                Vec::new(),
                None,
            )
            .await
            .map_err(|error| error.to_string())?;
        tracing::info!(%session_id, count = 0, "related tags: analysis complete");
        return Ok(());
    }
    let candidate_hits = app
        .tantivy()
        .related_documents(&source.combined, session_id, CANDIDATE_LIMIT)
        .await
        .map_err(|error| error.to_string())?;

    let mut candidates = Vec::new();
    for hit in candidate_hits {
        let Some(record) = store.session_get(&hit.id) else {
            continue;
        };
        if record.meta.tags.is_empty() {
            continue;
        }
        let candidate_source = source_content(store, &hit.id).await?;
        if !candidate_source.combined.is_empty() {
            candidates.push((record.meta.tags, candidate_source));
        }
    }

    let suggestions = rank_tags(&current.meta.tags, &source, candidates);
    let latest_source = source_content(store, session_id).await?;
    if queue.revision(session_id) != analysis_revision || latest_source.hash != source.hash {
        tracing::debug!(%session_id, "related tags: discarded stale analysis");
        return Ok(());
    }
    let auto_accept = app.settings().config().auto_accept_related_tags;
    let completed = store
        .complete_tag_suggestions(
            session_id,
            &source.hash,
            ALGORITHM_VERSION,
            suggestions.clone(),
            auto_accept.then_some(AUTO_ACCEPT_THRESHOLD),
        )
        .await
        .map_err(|error| error.to_string())?;

    if completed && auto_accept {
        for suggestion in suggestions
            .iter()
            .filter(|suggestion| suggestion.confidence >= AUTO_ACCEPT_THRESHOLD)
        {
            if let Err(error) = store.ensure_tag(&suggestion.name).await {
                tracing::warn!(tag = %suggestion.name, %error, "related tags: registry sync failed");
            }
        }
    }
    tracing::info!(%session_id, count = suggestions.len(), "related tags: analysis complete");
    Ok(())
}

#[derive(Clone, Default)]
struct SourceContent {
    transcript: String,
    summary: String,
    note: String,
    combined: String,
    hash: String,
}

async fn source_content(store: &SessionStore, session_id: &str) -> Result<SourceContent, String> {
    let record = store
        .session_get(session_id)
        .ok_or_else(|| format!("session {session_id} disappeared before analysis"))?;
    let transcripts = store
        .session_transcripts(session_id)
        .await
        .map_err(|error| error.to_string())?;
    let transcript = transcripts
        .iter()
        .flat_map(|transcript| transcript.words.iter())
        .map(|word| word.text.trim())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let note = record
        .note_markdown
        .as_deref()
        .map(crate::search_index::extract_plain_text)
        .unwrap_or_default();
    let summary = store
        .session_enhanced_docs(session_id)
        .iter()
        .map(|doc| crate::search_index::extract_plain_text(&doc.markdown))
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(assemble_source_content(transcript, summary, note))
}

fn assemble_source_content(transcript: String, summary: String, note: String) -> SourceContent {
    let combined = [&transcript, &summary, &note]
        .into_iter()
        .map(|content| content.trim())
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut hasher = Sha256::new();
    for content in [&transcript, &summary, &note] {
        hasher.update((content.len() as u64).to_le_bytes());
        hasher.update(content.as_bytes());
    }
    let hash = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    SourceContent {
        transcript,
        summary,
        note,
        combined,
        hash,
    }
}

fn rank_tags(
    attached_tags: &[String],
    target: &SourceContent,
    candidates: Vec<(Vec<String>, SourceContent)>,
) -> Vec<TagSuggestionItem> {
    if candidates.is_empty() {
        return Vec::new();
    }
    let target_terms = weighted_term_frequencies(target);
    if target_terms.len() < 10 {
        return Vec::new();
    }
    let candidate_terms: Vec<_> = candidates
        .iter()
        .map(|(_, source)| weighted_term_frequencies(source))
        .collect();
    let mut document_frequency = HashMap::<String, usize>::new();
    for terms in std::iter::once(&target_terms).chain(candidate_terms.iter()) {
        for term in terms.keys() {
            *document_frequency.entry(term.clone()).or_default() += 1;
        }
    }
    let document_count = candidate_terms.len() + 1;
    let attached: HashSet<_> = attached_tags.iter().cloned().collect();
    let mut evidence = HashMap::<String, Vec<f32>>::new();

    for ((tags, _), terms) in candidates.iter().zip(candidate_terms.iter()) {
        let similarity = cosine_tfidf(&target_terms, terms, &document_frequency, document_count);
        if similarity < 0.15 {
            continue;
        }
        for tag in tags {
            let Some(tag) = hypr_vault_read::normalize_tag_name(tag) else {
                continue;
            };
            if !attached.contains(&tag) {
                evidence.entry(tag).or_default().push(similarity);
            }
        }
    }

    let mut suggestions: Vec<TagSuggestionItem> = evidence
        .into_iter()
        .filter_map(|(name, similarities)| {
            let strongest = similarities.iter().copied().fold(0.0_f32, f32::max);
            let mut confidence = 1.0
                - similarities
                    .iter()
                    .fold(1.0_f32, |remaining, score| remaining * (1.0 - score));
            if similarities.len() == 1 && strongest < 0.9 {
                confidence = confidence.min(AUTO_ACCEPT_THRESHOLD - 0.01);
            }
            (confidence >= SUGGESTION_THRESHOLD).then_some(TagSuggestionItem { name, confidence })
        })
        .collect();
    suggestions.sort_by(|left, right| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then_with(|| left.name.cmp(&right.name))
    });
    suggestions.truncate(SUGGESTION_LIMIT);
    suggestions
}

fn term_frequencies(text: &str) -> HashMap<String, usize> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter_map(|term| {
            let term = term.to_lowercase();
            (term.chars().count() >= 3 && !STOP_WORDS.contains(&term.as_str())).then_some(term)
        })
        .fold(HashMap::new(), |mut frequencies, term| {
            *frequencies.entry(term).or_default() += 1;
            frequencies
        })
}

fn weighted_term_frequencies(source: &SourceContent) -> HashMap<String, f32> {
    let mut weighted = HashMap::new();
    for (content, source_weight) in [
        (&source.transcript, TRANSCRIPT_WEIGHT),
        (&source.summary, SUMMARY_WEIGHT),
        (&source.note, NOTE_WEIGHT),
    ] {
        for (term, frequency) in term_frequencies(content) {
            let frequency_weight = 1.0 + (frequency as f32).ln();
            *weighted.entry(term).or_default() += source_weight * frequency_weight;
        }
    }
    weighted
}

fn cosine_tfidf(
    left: &HashMap<String, f32>,
    right: &HashMap<String, f32>,
    document_frequency: &HashMap<String, usize>,
    document_count: usize,
) -> f32 {
    let weight = |term: &str, frequency: f32| {
        let df = document_frequency.get(term).copied().unwrap_or(0) as f32;
        let idf = ((document_count as f32 + 1.0) / (df + 1.0)).ln() + 1.0;
        frequency * idf
    };
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for (term, frequency) in left {
        let left_weight = weight(term, *frequency);
        left_norm += left_weight * left_weight;
        if let Some(right_frequency) = right.get(term) {
            dot += left_weight * weight(term, *right_frequency);
        }
    }
    for (term, frequency) in right {
        let right_weight = weight(term, *frequency);
        right_norm += right_weight * right_weight;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    dot / (left_norm.sqrt() * right_norm.sqrt())
}

const STOP_WORDS: &[&str] = &[
    "and", "are", "but", "for", "from", "have", "that", "the", "their", "this", "was", "were",
    "will", "with", "you", "your",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn source(transcript: &str, summary: &str, note: &str) -> SourceContent {
        assemble_source_content(
            transcript.to_string(),
            summary.to_string(),
            note.to_string(),
        )
    }

    #[test]
    fn recurring_topic_transfers_existing_tags() {
        let suggestions = rank_tags(
            &[],
            &source(
                "atlas launch rollout customer acme migration timeline launch rollout readiness deployment milestones ownership support",
                "",
                "",
            ),
            vec![
                (
                    vec!["project/atlas".to_string(), "customer/acme".to_string()],
                    source(
                        "atlas launch rollout customer acme migration timeline rollout readiness deployment milestones ownership support",
                        "",
                        "",
                    ),
                ),
                (
                    vec!["project/atlas".to_string()],
                    source(
                        "atlas rollout launch readiness migration customer acme timeline deployment milestones ownership support",
                        "",
                        "",
                    ),
                ),
                (
                    vec!["hiring".to_string()],
                    source(
                        "candidate interview frontend engineering feedback unrelated topic",
                        "",
                        "",
                    ),
                ),
            ],
        );

        assert_eq!(suggestions[0].name, "project/atlas");
        assert!(suggestions.iter().any(|item| item.name == "customer/acme"));
        assert!(!suggestions.iter().any(|item| item.name == "hiring"));
    }

    #[test]
    fn attached_and_unrelated_tags_are_omitted() {
        let suggestions = rank_tags(
            &["project/atlas".to_string()],
            &source(
                "atlas launch rollout customer acme migration timeline launch rollout readiness deployment milestones ownership support",
                "",
                "",
            ),
            vec![(
                vec!["project/atlas".to_string(), "hiring".to_string()],
                source(
                    "candidate interview frontend engineering feedback unrelated topic",
                    "",
                    "",
                ),
            )],
        );

        assert!(suggestions.is_empty());
    }

    #[test]
    fn note_and_summary_terms_match_transcript_terms() {
        let suggestions = rank_tags(
            &[],
            &source(
                "",
                "Atlas deployment readiness and customer migration milestones",
                "Acme rollout owners need training support escalation timeline confirmation",
            ),
            vec![(
                vec!["project/atlas".to_string()],
                source(
                    "Atlas rollout for Acme customer deployment readiness migration milestones owners training support escalation timeline confirmation",
                    "",
                    "",
                ),
            )],
        );

        assert_eq!(suggestions[0].name, "project/atlas");
    }

    #[test]
    fn source_hash_changes_for_each_content_kind() {
        let baseline = source("atlas transcript", "atlas summary", "atlas note");
        assert_ne!(
            baseline.hash,
            source("changed transcript", "atlas summary", "atlas note").hash
        );
        assert_ne!(
            baseline.hash,
            source("atlas transcript", "changed summary", "atlas note").hash
        );
        assert_ne!(
            baseline.hash,
            source("atlas transcript", "atlas summary", "changed note").hash
        );
        assert_eq!(
            baseline.hash,
            source("atlas transcript", "atlas summary", "atlas note").hash
        );
    }

    #[test]
    fn note_changes_debounce_until_idle_but_respect_max_wait() {
        let start = Instant::now();
        let mut state = DebounceState::default();
        let (first_generation, first_delay) = state.schedule(
            "s1",
            start,
            Duration::from_secs(10),
            Duration::from_secs(30),
        );
        assert_eq!(first_delay, Duration::from_secs(10));
        assert_eq!(state.revision("s1"), 1);

        let (second_generation, second_delay) = state.schedule(
            "s1",
            start + Duration::from_secs(8),
            Duration::from_secs(10),
            Duration::from_secs(30),
        );
        assert_eq!(second_delay, Duration::from_secs(10));
        assert_eq!(state.revision("s1"), 2);
        assert!(!state.take_if_current("s1", first_generation));

        let (third_generation, third_delay) = state.schedule(
            "s1",
            start + Duration::from_secs(25),
            Duration::from_secs(10),
            Duration::from_secs(30),
        );
        assert_eq!(third_delay, Duration::from_secs(5));
        assert_eq!(state.revision("s1"), 3);
        assert!(!state.take_if_current("s1", second_generation));
        assert!(state.take_if_current("s1", third_generation));
    }
}
