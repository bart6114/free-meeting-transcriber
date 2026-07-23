use std::collections::HashMap;

// These types used to be re-exported from the `hypr-analytics` (PostHog)
// crate. That crate is gone now that analytics is a no-op, but the shapes
// are kept here verbatim so the generated TypeScript bindings — and the
// small number of in-process Rust callers (`plugins/notification`,
// `plugins/windows`) — keep compiling unchanged.

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AnalyticsPayload {
    pub event: String,
    #[serde(flatten)]
    pub props: HashMap<String, serde_json::Value>,
}

#[derive(Clone)]
pub struct AnalyticsPayloadBuilder {
    event: Option<String>,
    props: HashMap<String, serde_json::Value>,
}

impl AnalyticsPayload {
    pub fn builder(event: impl Into<String>) -> AnalyticsPayloadBuilder {
        AnalyticsPayloadBuilder {
            event: Some(event.into()),
            props: HashMap::new(),
        }
    }
}

impl AnalyticsPayloadBuilder {
    pub fn with(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.props.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> AnalyticsPayload {
        AnalyticsPayload {
            event: self.event.expect("'event' is not specified"),
            props: self.props,
        }
    }
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PropertiesPayload {
    #[serde(default)]
    pub set: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub set_once: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

#[derive(Default)]
pub struct PropertiesPayloadBuilder {
    set: HashMap<String, serde_json::Value>,
    set_once: HashMap<String, serde_json::Value>,
}

impl PropertiesPayload {
    pub fn builder() -> PropertiesPayloadBuilder {
        PropertiesPayloadBuilder::default()
    }
}

impl PropertiesPayloadBuilder {
    pub fn set(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.set.insert(key.into(), value.into());
        self
    }

    pub fn set_once(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.set_once.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> PropertiesPayload {
        PropertiesPayload {
            set: self.set,
            set_once: self.set_once,
            email: None,
            user_id: None,
        }
    }
}
