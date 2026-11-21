//! A single draft holds both verdict forms and a shared free-form response.
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Field, Form, Input};
use crate::{
    human,
    runtime::Verdict,
    store::{Receipts, Request},
    types::{RequestId, ReviewerId},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum FieldValue {
    Fixed,
    Bool(Option<bool>),
    Choice(Option<u32>),
    Text(String),
    Integer(String),
    Number(String),
    List(Vec<String>),
    Groups(Vec<DraftFields>),
}

pub type DraftFields = Vec<(String, FieldValue)>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub verdict: Option<Verdict>,
    pub green: DraftFields,
    pub red: DraftFields,
    pub notes: String,
}

/// Changes are applied to the saved draft, not a caller's potentially stale snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DraftEdit {
    Verdict {
        verdict: Verdict,
    },
    Field {
        verdict: Verdict,
        name: String,
        value: FieldValue,
    },
    AddItem {
        verdict: Verdict,
        name: String,
    },
    RemoveItem {
        verdict: Verdict,
        name: String,
        item: u32,
    },
    /// Rebase over Points appended since the editor read the saved text.
    Notes {
        previous: String,
        text: String,
    },
}

impl Draft {
    /// Rebuild one verdict form from the recorded schema and this draft's field inputs.
    pub fn form(&self, request: &Request, verdict: Verdict) -> Result<Form, String> {
        let mut form = super::form(request, verdict)?;
        fill(&mut form.fields, self.fields(verdict));
        Ok(form)
    }

    pub fn fields(&self, verdict: Verdict) -> &DraftFields {
        match verdict {
            Verdict::Green => &self.green,
            Verdict::Red => &self.red,
        }
    }

    fn fields_mut(&mut self, verdict: Verdict) -> &mut DraftFields {
        match verdict {
            Verdict::Green => &mut self.green,
            Verdict::Red => &mut self.red,
        }
    }

    pub(crate) fn edit(&mut self, request: &Request, edit: DraftEdit) -> Result<(), String> {
        match edit {
            DraftEdit::Verdict { verdict } => self.verdict = Some(verdict),
            DraftEdit::Notes { previous, text } => {
                let suffix = self
                    .notes
                    .strip_prefix(&previous)
                    .ok_or("Review notes changed; reload the draft before saving.")?;
                self.notes = text + suffix;
            }
            DraftEdit::Field {
                verdict,
                name,
                value,
            } => {
                let mut form = self.form(request, verdict)?;
                let field = form
                    .fields
                    .iter_mut()
                    .find(|field| field.name == name)
                    .ok_or_else(|| format!("{name} is not a field of this form."))?;
                fill(std::slice::from_mut(field), &[(name, value)]);
                *self.fields_mut(verdict) = values_of(&form.fields);
            }
            DraftEdit::AddItem { verdict, name } => {
                self.edit_list(request, verdict, &name, Form::add)?;
            }
            DraftEdit::RemoveItem {
                verdict,
                name,
                item,
            } => {
                self.edit_list(request, verdict, &name, |form, index| {
                    form.remove(index, item as usize)
                })?;
            }
        }
        Ok(())
    }

    fn edit_list(
        &mut self,
        request: &Request,
        verdict: Verdict,
        name: &str,
        edit: impl FnOnce(&mut Form, usize),
    ) -> Result<(), String> {
        let mut form = self.form(request, verdict)?;
        let index = form
            .fields
            .iter()
            .position(|field| {
                field.name == name && matches!(field.input, Input::List(_) | Input::Groups { .. })
            })
            .ok_or_else(|| format!("{name} is not a list of this form."))?;
        edit(&mut form, index);
        *self.fields_mut(verdict) = values_of(&form.fields);
        Ok(())
    }

    pub(crate) fn append(&mut self, text: &str) {
        if !self.notes.is_empty() && !self.notes.ends_with('\n') {
            self.notes.push('\n');
        }
        self.notes.push_str(text);
    }
}

/// Restore partial or invalid input as typed; only submission validates the answer schema.
fn fill(fields: &mut [Field], values: &[(String, FieldValue)]) {
    for field in fields {
        let Some((_, value)) = values.iter().find(|(name, _)| *name == field.name) else {
            continue;
        };
        match (&mut field.input, value) {
            (Input::Bool(current), FieldValue::Bool(value)) => *current = *value,
            (Input::Choice(choice), FieldValue::Choice(index)) => {
                choice.select(index.map(|index| index as usize));
            }
            (Input::Text(current), FieldValue::Text(text))
            | (Input::Integer(current), FieldValue::Integer(text))
            | (Input::Number(current), FieldValue::Number(text)) => current.clone_from(text),
            (Input::List(current), FieldValue::List(items)) => current.clone_from(items),
            (Input::Groups { fields, items }, FieldValue::Groups(values)) => {
                *items = values
                    .iter()
                    .map(|values| {
                        let mut item = fields.clone();
                        fill(&mut item, values);
                        item
                    })
                    .collect();
            }
            // A constant stays fixed; a value of another kind is not this field's.
            _ => {}
        }
    }
}

pub fn values_of(fields: &[Field]) -> DraftFields {
    fields
        .iter()
        .map(|field| {
            let value = match &field.input {
                Input::Fixed(_) => FieldValue::Fixed,
                Input::Bool(value) => FieldValue::Bool(*value),
                Input::Choice(choice) => {
                    FieldValue::Choice(choice.selected().map(|index| index as u32))
                }
                Input::Text(text) => FieldValue::Text(text.clone()),
                Input::Integer(text) => FieldValue::Integer(text.clone()),
                Input::Number(text) => FieldValue::Number(text.clone()),
                Input::List(items) => FieldValue::List(items.clone()),
                Input::Groups { items, .. } => {
                    FieldValue::Groups(items.iter().map(|item| values_of(item)).collect())
                }
            };
            (field.name.clone(), value)
        })
        .collect()
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
            .change_review_draft(request, reviewer, |request, draft| {
                draft.edit(request, edit)
            })
            .await
    })
    .await
}

/// Append a Point as its own line, preserving the existing notes and verdict fields. Only the
/// current claimant can append; the claim check and the append are one write transaction.
pub async fn append_draft(
    state: &Path,
    request: &RequestId,
    reviewer: &ReviewerId,
    text: &str,
) -> Result<Draft, String> {
    if text.trim().is_empty() {
        return Err("There is no text to append to the review draft.".into());
    }
    let text = text.to_owned();
    with_receipts(state, request, async |receipts| {
        receipts
            .change_review_draft(request, reviewer, move |_, draft| {
                draft.append(&text);
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
