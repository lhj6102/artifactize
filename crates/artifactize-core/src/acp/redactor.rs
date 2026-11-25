//! Line-preserving cross-chunk secret redactor confined inside the ACP boundary (`C-ACP-EDGE-ISOLATION`).

/// Placeholder substituted inline by [`StreamRedactor`] while preserving line structure.
pub(crate) const REDACTED: &str = "[REDACTED]";

/// Line-preserving cross-chunk secret redactor for ACP stream deltas, tool outputs, diffs,
/// answers, handoffs, RPC errors, and stderr. Keeps a sliding tail of `max_known_secret_len - 1`
/// characters across chunk boundaries so a secret split over two chunks is still redacted before
/// persistence. Unknown credentials read directly from disk by an agent process cannot be
/// detected here.
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

#[cfg(test)]
mod tests {
    use super::*;

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
