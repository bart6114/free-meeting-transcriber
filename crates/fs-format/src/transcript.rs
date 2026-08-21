use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use specta::Type;

fn null_or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct TranscriptJson {
    #[serde(default, deserialize_with = "null_or_default")]
    pub transcripts: Vec<TranscriptWithData>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct TranscriptWithData {
    pub id: String,
    #[serde(default, deserialize_with = "null_or_default")]
    pub user_id: String,
    #[serde(default, deserialize_with = "null_or_default")]
    pub created_at: String,
    pub session_id: String,
    #[serde(default, deserialize_with = "null_or_default")]
    pub started_at: f64,
    #[serde(default)]
    pub ended_at: Option<f64>,
    #[serde(default, deserialize_with = "null_or_default")]
    pub memo_md: String,
    #[serde(default, deserialize_with = "null_or_default")]
    pub words: Vec<TranscriptWord>,
    #[serde(default, deserialize_with = "null_or_default")]
    pub speaker_hints: Vec<TranscriptSpeakerHint>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct TranscriptWord {
    #[serde(default)]
    pub id: Option<String>,
    pub text: String,
    pub start_ms: f64,
    pub end_ms: f64,
    pub channel: f64,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub metadata: Option<Map<String, Value>>,
}

/// Word-free mirror of `TranscriptJson` for callers that need counts, not content:
/// deserializing through this lexes the whole file but allocates nothing per word,
/// which is what keeps summarizing a large vault cheap.
#[derive(Debug, serde::Deserialize)]
pub struct TranscriptJsonStats {
    #[serde(default, deserialize_with = "null_or_default")]
    pub transcripts: Vec<TranscriptStat>,
}

#[derive(Debug, serde::Deserialize)]
pub struct TranscriptStat {
    pub id: String,
    #[serde(default, deserialize_with = "null_or_default")]
    pub words: SeqCount,
}

/// Deserializes a JSON array (or `null`, per the file format's `null_or_default`
/// tolerance) by counting its elements and discarding them.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct SeqCount(pub usize);

impl<'de> Deserialize<'de> for SeqCount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = SeqCount;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an array or null")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut count = 0;
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    count += 1;
                }
                Ok(SeqCount(count))
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(SeqCount(0))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct TranscriptSpeakerHint {
    #[serde(default)]
    pub id: Option<String>,
    pub word_id: String,
    #[serde(rename = "type")]
    pub hint_type: String,
    #[serde(default, deserialize_with = "null_or_default")]
    pub value: Value,
}
