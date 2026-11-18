//! Provider error bodies decoded once, with the common error envelope and the additional
//! Codex Responses fields used for diagnostics and classification.
use crate::json::{Object, Optional};
use reqwest::StatusCode;
use serde::{Deserialize, Deserializer};

#[derive(Default, Deserialize)]
pub(super) struct Fields {
    #[serde(default)]
    code: Optional<String>,
    #[serde(default)]
    status: Optional<String>,
    #[serde(default, rename = "type")]
    kind: Optional<String>,
    #[serde(default)]
    message: Message,
    #[serde(default)]
    pub plan_type: Optional<String>,
    /// When the usage limit resets: whole seconds since the Unix epoch on the wire.
    #[serde(default)]
    pub resets_at: Optional<crate::auth::codex::Timestamp>,
}
// An explicitly present non-string message must not make a nested error's message
// fall through to a top-level message. Only an absent nested field does that.
#[derive(Default)]
struct Message(Option<Optional<String>>);
impl<'de> Deserialize<'de> for Message {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Optional::deserialize(deserializer).map(|value| Self(Some(value)))
    }
}
impl Message {
    fn text(&self) -> Option<&str> {
        self.0.as_ref()?.0.as_deref()
    }
}

#[derive(Default, Deserialize)]
struct Response {
    #[serde(default)]
    error: Optional<Object<Fields>>,
}
#[derive(Default, Deserialize)]
struct Envelope {
    #[serde(default)]
    response: Optional<Object<Response>>,
    #[serde(default)]
    error: Optional<Object<Fields>>,
    #[serde(default)]
    detail: Optional<String>,
    #[serde(flatten)]
    fields: Fields,
}
pub(super) struct Body(Envelope);

impl Body {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let Object(envelope) = serde_json::from_slice::<Object<Envelope>>(bytes).ok()?;
        Some(Self(envelope))
    }

    /// The generic provider code comes from the top-level error object only.
    pub fn provider_code(&self) -> Option<&str> {
        let Object(fields) = self.0.error.0.as_ref()?;
        [&fields.code, &fields.status, &fields.kind]
            .into_iter()
            .filter_map(|field| field.0.as_deref())
            .find(|code| !code.is_empty())
    }

    pub fn provider_message(&self) -> Option<&str> {
        self.0
            .error
            .0
            .as_ref()
            .map(|Object(fields)| &fields.message)
            .filter(|message| message.0.is_some())
            .unwrap_or(&self.0.fields.message)
            .text()
    }

    /// Responses streams may carry errors in `response.error` or directly at the root.
    pub fn codex_fields(&self) -> &Fields {
        self.0
            .response
            .0
            .as_ref()
            .and_then(|Object(response)| response.error.0.as_ref())
            .or(self.0.error.0.as_ref())
            .map_or(&self.0.fields, |Object(fields)| fields)
    }

    pub fn codex_message(&self) -> Option<&str> {
        self.codex_fields()
            .message
            .text()
            .or(self.0.detail.0.as_deref())
    }

    pub fn usage_limit(&self, status: Option<StatusCode>) -> bool {
        self.codex_fields()
            .code()
            .is_some_and(|code| matches!(code, "usage_limit_reached" | "usage_not_included"))
            || (status == Some(StatusCode::TOO_MANY_REQUESTS)
                && self.codex_fields().resets_at.0.is_some())
    }
}

impl Fields {
    pub fn code(&self) -> Option<&str> {
        self.code
            .0
            .as_deref()
            .or(self.kind.0.as_deref())
            .filter(|code| !code.is_empty())
    }
}
