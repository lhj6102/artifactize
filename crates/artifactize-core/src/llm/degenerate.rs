//! A response whose visible text has degenerated into whitespace: the model stopped its
//! answer and streams nothing else until the output cap or the connection ends.

use crate::agent::error::{Code, Failure};

/// Whitespace characters in a row at the end of a response's text that stop its stream.
/// Degenerated responses seen in reviews streamed 157,000 to 7,150,000 of them after an
/// unfinished result, until the provider's output cap or a broken connection, minutes later.
/// No answer holds this many in a row, so stopping here keeps all output that has meaning
/// and ends the attempt in seconds.
pub(super) const WHITESPACE_RUN: usize = 4096;

/// The whitespace run at the end of the text streamed so far.
#[derive(Default)]
pub(super) struct Tail {
    run: usize,
}

impl Tail {
    /// Add a streamed text delta: the failure that stops the stream once the text ends in
    /// [`WHITESPACE_RUN`] whitespace characters.
    pub fn push(&mut self, text: &str) -> Result<(), Failure> {
        let trailing = text.chars().rev().take_while(|c| c.is_whitespace()).count();
        self.run = if trailing == text.chars().count() {
            self.run.saturating_add(trailing)
        } else {
            trailing
        };
        if self.run < WHITESPACE_RUN {
            return Ok(());
        }
        Err(Failure::new(
            Code::DegenerateOutput,
            format!(
                "Provider response degenerated: its text ended in {} whitespace characters, so artifactize stopped the stream.",
                self.run
            ),
        ))
    }
}
