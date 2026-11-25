//! The one rule for server text that is about to be shown, saved or logged while a request
//! carried a credential (a bearer token, an API key): a store, provider or proxy may echo the
//! credential back, whole or split, inside an error. Control characters go first (so a split
//! cannot be rejoined by dropping them afterwards), then text holding any piece of a
//! credential is withheld whole, and only then is the rest clipped.

/// Consecutive characters of a credential that count as a piece of it. Eight characters of a
/// random token are already too many to guess from, while shorter runs (a `sk-` prefix, a common
/// word) occur in ordinary error text and would withhold harmless messages.
const PIECE: usize = 8;

/// What is shown instead of server text that held part of a credential.
pub(crate) const WITHHELD: &str = "[withheld: it contained part of the credential sent]";

/// Placeholder substituted inline by [`StreamRedactor`] while preserving line structure.
pub(crate) const REDACTED: &str = "[REDACTED]";

/// Line-preserving cross-chunk secret redactor for ACP stream deltas, tool outputs, diffs,
/// answers, handoffs and stderr. Keeps a sliding tail of `max_known_secret_len - 1` characters
/// across chunk boundaries so a secret split over two chunks is still redacted before persistence.
/// Unknown credentials read directly from disk by an agent process cannot be detected here.
#[derive(Debug, Clone)]
pub(crate) struct StreamRedactor {
    secrets: Vec<String>,
    window_chars: usize,
    pending: String,
}

impl StreamRedactor {
    pub(crate) fn new(secrets: impl IntoIterator<Item = String>) -> Self {
        let mut unique: Vec<String> = secrets
            .into_iter()
            .filter(|secret| !secret.is_empty())
            .collect();
        unique.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        unique.dedup();
        let window_chars = unique
            .iter()
            .map(|secret| secret.chars().count())
            .max()
            .unwrap_or(0)
            .saturating_sub(1);
        Self {
            secrets: unique,
            window_chars,
            pending: String::new(),
        }
    }

    /// Push a chunk and emit the safe prefix whose characters can no longer complete a secret.
    pub(crate) fn push(&mut self, chunk: &str) -> String {
        if self.secrets.is_empty() {
            return chunk.to_owned();
        }
        self.pending.push_str(chunk);
        self.redact_pending();
        let total = self.pending.chars().count();
        if total <= self.window_chars {
            return String::new();
        }
        let emit_chars = total - self.window_chars;
        let split_byte = self
            .pending
            .char_indices()
            .nth(emit_chars)
            .map_or(self.pending.len(), |(index, _)| index);
        let tail = self.pending.split_off(split_byte);
        std::mem::replace(&mut self.pending, tail)
    }

    /// Flush the trailing sliding window at segment or turn end.
    pub(crate) fn flush(&mut self) -> String {
        if self.secrets.is_empty() {
            return std::mem::take(&mut self.pending);
        }
        self.redact_pending();
        std::mem::take(&mut self.pending)
    }

    /// Redact a complete string in one shot while preserving newlines.
    pub(crate) fn redact_all(&self, text: &str) -> String {
        if self.secrets.is_empty() || text.is_empty() {
            return text.to_owned();
        }
        let mut copy = Self {
            secrets: self.secrets.clone(),
            window_chars: self.window_chars,
            pending: String::new(),
        };
        let mut out = copy.push(text);
        out.push_str(&copy.flush());
        out
    }

    fn redact_pending(&mut self) {
        for secret in &self.secrets {
            if self.pending.contains(secret.as_str()) {
                self.pending = self.pending.replace(secret.as_str(), REDACTED);
            }
        }
    }
}

/// `text` from a server, safe to show: no control characters, no piece of `secrets`, at most
/// `max_chars` characters.
pub(crate) fn server_text(text: &str, secrets: &[&str], max_chars: usize) -> String {
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    if secrets.iter().any(|secret| holds_piece(&text, secret)) {
        return WITHHELD.to_owned();
    }
    text.chars().take(max_chars).collect()
}

/// `text` contains `PIECE` consecutive characters of `secret` (all of a shorter one).
fn holds_piece(text: &str, secret: &str) -> bool {
    let secret: Vec<char> = secret.chars().filter(|c| !c.is_control()).collect();
    if secret.is_empty() {
        return false;
    }
    let size = PIECE.min(secret.len());
    secret
        .windows(size)
        .any(|piece| text.contains(&piece.iter().collect::<String>()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "azt_0123456789abcdef0123456789abcdef";

    #[test]
    fn text_holding_any_piece_of_a_credential_is_withheld() {
        let (head, tail) = TOKEN.split_at(12);
        for text in [
            format!("Bad request; Authorization: Bearer {TOKEN}"),
            // Split by a control character, which dropping control characters would rejoin.
            format!("Authorization: Bearer {head}\n{tail}"),
            format!("{head}\u{1b}[0m{tail}"),
            // At the clipping boundary: the piece before the clip is enough.
            format!("{}{TOKEN}", "x".repeat(295)),
            format!("{}{}", "x".repeat(299), &TOKEN[..8]),
            // Only a piece of it.
            format!("unknown field `{}`", &TOKEN[20..30]),
        ] {
            assert_eq!(
                server_text(&text, &[TOKEN], 300),
                "[withheld: it contained part of the credential sent]",
                "{text:?}"
            );
        }
    }

    #[test]
    fn other_text_loses_control_characters_and_is_clipped() {
        assert_eq!(
            server_text("Unknown\u{7} schema.\n", &[TOKEN], 300),
            "Unknown schema."
        );
        assert_eq!(server_text(&"y".repeat(500), &[TOKEN], 300).len(), 300);
        // Seven characters of it are not a piece.
        assert_eq!(server_text(&TOKEN[..7], &[TOKEN], 300), &TOKEN[..7]);
        assert_eq!(server_text("plain", &[], 300), "plain");
        assert_eq!(
            server_text("ab", &["ab"], 300),
            "[withheld: it contained part of the credential sent]"
        );
    }

    #[test]
    fn stream_redactor_preserves_lines_and_catches_cross_chunk_secrets() {
        let mut redactor = StreamRedactor::new(["super-secret-token-123".to_owned()]);
        let mut output = String::new();
        for chunk in ["line1\nkey=super-", "secret-", "token-123\nline3"] {
            output.push_str(&redactor.push(chunk));
        }
        output.push_str(&redactor.flush());
        assert_eq!(output, "line1\nkey=[REDACTED]\nline3");
    }
}
