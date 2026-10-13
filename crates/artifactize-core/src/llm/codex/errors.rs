//! Codex error bodies parsed at the provider boundary; unknown and wrong-typed optional
//! diagnostics retain the endpoint's former fallback behavior.
use crate::json::{Object, Optional};
use serde::Deserialize;

#[derive(Default, Deserialize)]
pub(super) struct Fields {
    #[serde(default)]
    pub code: Optional<String>,
    #[serde(default, rename = "type")]
    pub kind: Optional<String>,
    #[serde(default)]
    pub message: Optional<String>,
    #[serde(default)]
    pub plan_type: Optional<String>,
    /// When the usage limit resets: whole seconds since the Unix epoch on the wire.
    #[serde(default)]
    pub resets_at: Optional<crate::auth::codex::Timestamp>,
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
pub(super) struct Error {
    pub fields: Fields,
    pub detail: Option<String>,
}
impl Error {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let Object(envelope) = serde_json::from_slice::<Object<Envelope>>(bytes).ok()?;
        let nested = envelope
            .response
            .0
            .and_then(|Object(response)| response.error.0);
        let fields = nested
            .or(envelope.error.0)
            .map_or(envelope.fields, |Object(fields)| fields);
        Some(Self {
            fields,
            detail: envelope.detail.0,
        })
    }
    pub fn code(&self) -> Option<&str> {
        self.fields
            .code
            .0
            .as_deref()
            .or(self.fields.kind.0.as_deref())
            .filter(|code| !code.is_empty())
    }
    pub fn usage_limit(&self, status: Option<u16>) -> bool {
        self.code()
            .is_some_and(|code| matches!(code, "usage_limit_reached" | "usage_not_included"))
            || (status == Some(429) && self.fields.resets_at.0.is_some())
    }
}
