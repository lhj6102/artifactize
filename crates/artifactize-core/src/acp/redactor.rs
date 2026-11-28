//! Line-preserving cross-chunk secret redactor confined inside the ACP boundary (`C-ACP-EDGE-ISOLATION`).

/// Placeholder substituted inline by [`StreamRedactor`] while preserving line structure.
pub(crate) const REDACTED: &str = "[REDACTED]";

/// Line-preserving cross-chunk secret redactor for ACP stream deltas, tool outputs, diffs,
/// answers, handoffs, RPC errors, and stderr. Holds any trailing substring that could still begin
/// or extend into a longer known secret across chunk boundaries so neither split secrets nor
/// prefix-overlapping secrets leak unredacted. Unknown credentials read directly from disk by an
/// agent process cannot be detected here.
#[derive(Debug, Clone)]
pub(crate) struct StreamRedactor {
    secrets: Vec<String>,
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
        Self {
            secrets: unique,
            pending: String::new(),
        }
    }

    /// Push a chunk and emit the safe prefix whose characters can no longer begin or extend into
    /// any known secret.
    pub(crate) fn push(&mut self, chunk: &str) -> String {
        if self.secrets.is_empty() {
            return chunk.to_owned();
        }
        self.pending.push_str(chunk);
        self.redact_streaming(false)
    }

    /// Emit what a display boundary inside the same stream (a tool call between two runs of
    /// prose, say) may show now: everything except any tail that could still begin or extend into
    /// a known secret. That tail stays pending so the stream's next chunk completes or clears it.
    pub(crate) fn release(&mut self) -> String {
        if self.secrets.is_empty() {
            return std::mem::take(&mut self.pending);
        }
        self.redact_streaming(false)
    }

    /// Whether any trailing characters are currently buffered awaiting subsequent chunks.
    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Flush the trailing sliding window at turn end.
    pub(crate) fn flush(&mut self) -> String {
        if self.secrets.is_empty() {
            return std::mem::take(&mut self.pending);
        }
        self.redact_streaming(true)
    }

    /// Redact a complete string in one shot while preserving newlines.
    pub(crate) fn redact_all(&self, text: &str) -> String {
        if self.secrets.is_empty() || text.is_empty() {
            return text.to_owned();
        }
        let mut copy = Self {
            secrets: self.secrets.clone(),
            pending: text.to_owned(),
        };
        copy.redact_streaming(true)
    }

    /// Left-to-right longest-match redaction. While streaming (`!final_flush`), stops before any
    /// suffix that is a proper prefix of some known secret so a shorter prefix secret is never
    /// replaced before we know whether the next chunk completes a longer secret.
    fn redact_streaming(&mut self, final_flush: bool) -> String {
        let mut out = String::new();
        let mut pos = 0;
        let len = self.pending.len();
        while pos < len {
            let rest = &self.pending[pos..];
            if !final_flush && self.could_extend_to_longer_secret(rest) {
                break;
            }
            if let Some(secret) = self.secrets.iter().find(|s| rest.starts_with(s.as_str())) {
                out.push_str(REDACTED);
                pos += secret.len();
            } else {
                let ch = rest.chars().next().expect("pos < len");
                out.push(ch);
                pos += ch.len_utf8();
            }
        }
        self.pending.drain(..pos);
        out
    }

    fn could_extend_to_longer_secret(&self, rest: &str) -> bool {
        self.secrets
            .iter()
            .any(|secret| secret.len() > rest.len() && secret.starts_with(rest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boundary_holds_back_only_what_could_begin_a_secret() {
        let mut redactor = StreamRedactor::new(["sk-0123456789".to_owned()]);
        let mut written = redactor.push("Reading the file then sk-0123");
        written.push_str(&redactor.release());
        assert!(!written.contains("sk-"), "{written}");
        written.push_str(" | tool | ");
        written.push_str(&redactor.push("456789 done"));
        written.push_str(&redactor.flush());
        assert_eq!(written, "Reading the file then  | tool | [REDACTED] done");

        let mut plain = StreamRedactor::new(["sk-0123456789".to_owned()]);
        let mut shown = plain.push("Nothing secret here.");
        shown.push_str(&plain.release());
        assert_eq!(shown, "Nothing secret here.");
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

    #[test]
    fn prefix_overlapping_secrets_do_not_leak_suffix_across_chunk_boundary() {
        let mut redactor =
            StreamRedactor::new(["sk-short".to_owned(), "sk-short-extended".to_owned()]);
        let mut output = redactor.push("val=sk-short");
        assert!(!output.contains("sk-short"), "{output}");
        output.push_str(&redactor.push("-extended ok"));
        output.push_str(&redactor.flush());
        assert_eq!(output, "val=[REDACTED] ok");
    }
}
