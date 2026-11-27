//! A single draft holds the reviewer's issues, the verdict chosen and the places a reviewer
//! pointed at.
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    human::{self, GREEN_WITH_ISSUES, RED_WITHOUT_ISSUE},
    runtime::Verdict,
    store::Receipts,
    types::{RequestId, ReviewerId, Timestamp},
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub verdict: Option<Verdict>,
    /// What must change, oldest first.
    #[serde(default)]
    pub issues: Vec<String>,
    #[serde(default)]
    pub points: Vec<Point>,
}

/// A location recorded alongside the draft, not part of the verdict's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub text: String,
    pub source: String,
    pub at: Timestamp,
}

impl Point {
    pub(crate) fn earlier_notes(text: String) -> Self {
        // Drafts used to have no timestamp. The epoch denotes an unknown original time.
        Self {
            text,
            source: "Earlier notes".into(),
            at: Timestamp::new(time::OffsetDateTime::UNIX_EPOCH).expect("epoch is sortable"),
        }
    }
}

/// Whether a verdict can be submitted, or the sentence saying what is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    Blocked(String),
}

/// Changes are applied to the saved draft, not a caller's potentially stale snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DraftEdit {
    Verdict {
        verdict: Verdict,
    },
    AddIssue {
        text: String,
    },
    /// Refused if issue `index` is no longer `previous`.
    EditIssue {
        index: u32,
        previous: String,
        text: String,
    },
    /// Refused if issue `index` is no longer `previous`.
    RemoveIssue {
        index: u32,
        previous: String,
    },
}

/// The refusal for an issue without text.
pub const EMPTY_ISSUE: &str = "An issue needs text; remove it instead.";
/// The refusal for changing an issue another surface changed first.
pub const ISSUE_CHANGED: &str = "This issue changed; reload the review draft.";

impl DraftEdit {
    /// Refuse an edit that is invalid whatever the saved draft holds.
    pub fn check(&self) -> Result<(), String> {
        match self {
            Self::AddIssue { text } | Self::EditIssue { text, .. } if text.trim().is_empty() => {
                Err(EMPTY_ISSUE.into())
            }
            _ => Ok(()),
        }
    }
}

impl Draft {
    /// Whether `verdict` can be submitted now: RED with an issue, GREEN without one.
    pub fn readiness(&self, verdict: Verdict) -> Readiness {
        match (verdict, self.issues.is_empty()) {
            (Verdict::Red, true) => Readiness::Blocked(RED_WITHOUT_ISSUE.into()),
            (Verdict::Green, false) => Readiness::Blocked(GREEN_WITH_ISSUES.into()),
            _ => Readiness::Ready,
        }
    }

    pub(crate) fn submission(&self) -> Result<super::Submission, String> {
        let verdict = self
            .verdict
            .ok_or("Choose GREEN or RED before submitting.")?;
        super::Submission::new(verdict, self.issues.clone())
    }

    /// Points can keep changing until settlement; a different answer must be submitted again.
    pub(crate) fn same_answer(&self, other: &Self) -> bool {
        self.verdict == other.verdict && self.issues == other.issues
    }

    pub fn edit(&mut self, edit: DraftEdit) -> Result<(), String> {
        edit.check()?;
        match edit {
            DraftEdit::Verdict { verdict } => self.verdict = Some(verdict),
            DraftEdit::AddIssue { text } => self.issues.push(text),
            DraftEdit::EditIssue {
                index,
                previous,
                text,
            } => *self.issue(index, &previous)? = text,
            DraftEdit::RemoveIssue { index, previous } => {
                self.issue(index, &previous)?;
                self.issues.remove(index as usize);
            }
        }
        Ok(())
    }

    /// Issue `index`, if it still reads `previous`.
    fn issue(&mut self, index: u32, previous: &str) -> Result<&mut String, String> {
        self.issues
            .get_mut(index as usize)
            .filter(|issue| *issue == previous)
            .ok_or_else(|| ISSUE_CHANGED.into())
    }
}

/// Read this reviewer's draft, including while the review is released.
pub async fn read_draft(
    state: &Path,
    request: &RequestId,
    reviewer: &ReviewerId,
) -> Result<Option<Draft>, String> {
    with_receipts(state, request, async |receipts| {
        receipts.read_review_draft(request, reviewer).await
    })
    .await
}

/// Apply an edit atomically, only while `reviewer` holds the active review.
pub async fn edit_draft(
    state: &Path,
    request: &RequestId,
    reviewer: &ReviewerId,
    edit: DraftEdit,
) -> Result<Draft, String> {
    with_receipts(state, request, async |receipts| {
        receipts
            .change_review_draft(request, reviewer, |_, draft| draft.edit(edit))
            .await
    })
    .await
}

/// Record one Point. The append and claim check share a write transaction.
pub async fn append_point(
    state: &Path,
    request: &RequestId,
    reviewer: &ReviewerId,
    text: &str,
    source: &str,
) -> Result<Draft, String> {
    if text.trim().is_empty() || source.trim().is_empty() {
        return Err("A review point needs text and a source.".into());
    }
    let text = text.to_owned();
    let source = source.to_owned();
    with_receipts(state, request, async |receipts| {
        receipts
            .change_review_draft(request, reviewer, move |_, draft| {
                draft.points.push(Point {
                    text,
                    source,
                    at: Timestamp::now(),
                });
                Ok(())
            })
            .await
    })
    .await
}

/// Remove the point the caller saw, not whichever point now occupies its old position.
pub async fn remove_point(
    state: &Path,
    request: &RequestId,
    reviewer: &ReviewerId,
    point: Point,
) -> Result<Draft, String> {
    with_receipts(state, request, async |receipts| {
        receipts
            .change_review_draft(request, reviewer, move |_, draft| {
                let index = draft
                    .points
                    .iter()
                    .position(|saved| *saved == point)
                    .ok_or("This point is no longer in the review draft.")?;
                draft.points.remove(index);
                Ok(())
            })
            .await
    })
    .await
}

async fn with_receipts<T>(
    state: &Path,
    request: &RequestId,
    work: impl AsyncFnOnce(&Receipts) -> Result<T, String>,
) -> Result<T, String> {
    let (receipts, _) = human::open(state, request).await?;
    let result = work(&receipts).await;
    let closed = receipts.close().await;
    result.and_then(|value| closed.map(|()| value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edited(edits: Vec<DraftEdit>) -> Result<Draft, String> {
        let mut draft = Draft::default();
        for edit in edits {
            draft.edit(edit)?;
        }
        Ok(draft)
    }

    #[test]
    fn green_takes_no_issues_and_red_needs_one() {
        let empty = Draft::default();
        assert_eq!(empty.readiness(Verdict::Green), Readiness::Ready);
        assert_eq!(empty.readiness(Verdict::Red), Readiness::Blocked(RED_WITHOUT_ISSUE.into()));
        let one = edited(vec![DraftEdit::AddIssue { text: "Fix the title".into() }]).unwrap();
        assert_eq!(one.readiness(Verdict::Red), Readiness::Ready);
        assert_eq!(one.readiness(Verdict::Green), Readiness::Blocked(GREEN_WITH_ISSUES.into()));
        assert!(one.submission().is_err(), "no verdict chosen");
    }

    #[test]
    fn issues_change_only_as_last_seen() {
        let mut draft = edited(vec![
            DraftEdit::AddIssue { text: "one".into() },
            DraftEdit::AddIssue { text: "two".into() },
        ])
        .unwrap();
        assert_eq!(
            draft.edit(DraftEdit::EditIssue { index: 0, previous: "two".into(), text: "x".into() }),
            Err(ISSUE_CHANGED.into())
        );
        draft
            .edit(DraftEdit::RemoveIssue { index: 0, previous: "one".into() })
            .unwrap();
        assert_eq!(draft.issues, ["two"]);
        assert_eq!(draft.edit(DraftEdit::AddIssue { text: " ".into() }), Err(EMPTY_ISSUE.into()));
    }

}
