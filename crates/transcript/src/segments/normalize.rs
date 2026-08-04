use std::collections::HashMap;

use crate::types::ChannelProfile;
use crate::types::{FinalizedWord, PartialWord};

use super::model::NormalizedWord;

/// A silence this long between words of the same channel closes the current
/// sentence unit even without sentence-final punctuation.
const SENTENCE_UNIT_MAX_GAP_MS: i64 = 10_000;

pub(super) fn normalize_words(
    final_words: &[FinalizedWord],
    partial_words: &[PartialWord],
    sentence_atomic: bool,
) -> Vec<NormalizedWord> {
    let mut combined: Vec<NormalizedWord> =
        Vec::with_capacity(final_words.len() + partial_words.len());

    combined.extend(final_words.iter().map(|word| NormalizedWord {
        text: word.text.clone(),
        start_ms: word.start_ms,
        end_ms: word.end_ms,
        channel: ChannelProfile::from(word.channel),
        is_final: true,
        id: Some(word.id.clone()),
        order: 0,
        speaker_index: word.speaker_index,
    }));

    combined.extend(partial_words.iter().map(|word| NormalizedWord {
        text: word.text.clone(),
        start_ms: word.start_ms,
        end_ms: word.end_ms,
        channel: ChannelProfile::from(word.channel),
        is_final: false,
        id: None,
        order: 0,
        speaker_index: word.speaker_index,
    }));

    combined.sort_by_key(|word| word.start_ms);

    if sentence_atomic {
        apply_sentence_atomic_order(&mut combined);
    }

    for (index, word) in combined.iter_mut().enumerate() {
        word.order = index;
    }

    combined
}

/// Reorder the time-sorted stream so each channel's sentences stay contiguous.
///
/// Words are grouped per channel into sentence units (closed by sentence-final
/// punctuation or a long silence), then the stream is stably re-sorted by each
/// word's unit start time. Within a channel the original word order is
/// preserved; across channels, interleaving happens only at unit boundaries.
fn apply_sentence_atomic_order(words: &mut Vec<NormalizedWord>) {
    let mut indices_by_channel: HashMap<ChannelProfile, Vec<usize>> = HashMap::new();
    for (index, word) in words.iter().enumerate() {
        indices_by_channel
            .entry(word.channel)
            .or_default()
            .push(index);
    }

    let mut unit_start_by_index: Vec<i64> = vec![0; words.len()];
    for indices in indices_by_channel.values() {
        let mut unit_start = 0;
        let mut previous_end = 0;
        let mut previous_closed = true;

        for &index in indices {
            let word = &words[index];
            if previous_closed || word.start_ms - previous_end > SENTENCE_UNIT_MAX_GAP_MS {
                unit_start = word.start_ms;
            }
            unit_start_by_index[index] = unit_start;
            previous_end = word.end_ms;
            previous_closed = ends_sentence(&word.text);
        }
    }

    let mut order: Vec<usize> = (0..words.len()).collect();
    order.sort_by_key(|&index| (unit_start_by_index[index], words[index].channel as i32));

    let reordered = order
        .into_iter()
        .map(|index| words[index].clone())
        .collect();
    *words = reordered;
}

fn ends_sentence(text: &str) -> bool {
    matches!(
        text.trim_end().chars().last(),
        Some('.' | '?' | '!' | '…' | '。' | '？' | '！')
    )
}
