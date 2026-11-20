//! GREEN/RED owner fields between the frontend's drafts and the review's [`Form`]. Which schema
//! gets which inputs, how values become the result and the local errors all stay in
//! [`artifactize_core::review::Form`]; a Human result schema only ever holds shapes a person
//! enters in a form.
use crate::Failure;
use artifactize_core::{
    review::{self, Field, Form, Input},
    runtime,
    store::Request,
};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Green,
    Red,
}

impl From<Verdict> for runtime::Verdict {
    fn from(verdict: Verdict) -> Self {
        match verdict {
            Verdict::Green => Self::Green,
            Verdict::Red => Self::Red,
        }
    }
}

impl From<runtime::Verdict> for Verdict {
    fn from(verdict: runtime::Verdict) -> Self {
        match verdict {
            runtime::Verdict::Green => Self::Green,
            runtime::Verdict::Red => Self::Red,
        }
    }
}

/// How one owner field is entered.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FieldInput {
    /// A `const`: filled in and fixed; `value` is its JSON.
    Fixed {
        value: String,
    },
    Bool,
    /// String `enum` options, as their JSON text.
    Choice {
        options: Vec<String>,
    },
    Text,
    Integer,
    Number,
    /// An array of strings, entered one item at a time.
    List,
    /// An array of flat objects: a group of these fields per item.
    Groups {
        fields: Vec<FormField>,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FormField {
    pub name: String,
    pub required: bool,
    /// The type and constraints, then any description.
    pub hint: String,
    pub input: FieldInput,
}

/// A new form: its typed fields, with the draft the library starts it from.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FormView {
    pub verdict: Verdict,
    pub fields: Vec<FormField>,
    pub draft: Draft,
}

/// A field's draft value, by the field's kind.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum FieldValue {
    Fixed,
    Bool(Option<bool>),
    /// The chosen option's index.
    Choice(Option<u32>),
    Text(String),
    Integer(String),
    Number(String),
    List(Vec<String>),
    /// Each group's field values by name.
    Groups(Vec<Vec<(String, FieldValue)>>),
}

impl From<FieldValue> for review::FieldValue {
    fn from(value: FieldValue) -> Self {
        match value {
            FieldValue::Fixed => Self::Fixed,
            FieldValue::Bool(value) => Self::Bool(value),
            FieldValue::Choice(value) => Self::Choice(value),
            FieldValue::Text(value) => Self::Text(value),
            FieldValue::Integer(value) => Self::Integer(value),
            FieldValue::Number(value) => Self::Number(value),
            FieldValue::List(value) => Self::List(value),
            FieldValue::Groups(groups) => Self::Groups(
                groups
                    .into_iter()
                    .map(|fields| {
                        fields
                            .into_iter()
                            .map(|(name, value)| (name, value.into()))
                            .collect()
                    })
                    .collect(),
            ),
        }
    }
}

impl From<review::FieldValue> for FieldValue {
    fn from(value: review::FieldValue) -> Self {
        match value {
            review::FieldValue::Fixed => Self::Fixed,
            review::FieldValue::Bool(value) => Self::Bool(value),
            review::FieldValue::Choice(value) => Self::Choice(value),
            review::FieldValue::Text(value) => Self::Text(value),
            review::FieldValue::Integer(value) => Self::Integer(value),
            review::FieldValue::Number(value) => Self::Number(value),
            review::FieldValue::List(value) => Self::List(value),
            review::FieldValue::Groups(groups) => Self::Groups(
                groups
                    .into_iter()
                    .map(|fields| {
                        fields
                            .into_iter()
                            .map(|(name, value)| (name, value.into()))
                            .collect()
                    })
                    .collect(),
            ),
        }
    }
}

/// The frontend's draft of one verdict: field values by name.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub verdict: Verdict,
    pub fields: Vec<(String, FieldValue)>,
}

fn new(request: &Request, verdict: Verdict) -> Result<Form, Failure> {
    review::form(request, verdict.into()).map_err(Failure::Action)
}

fn form_field(field: &Field) -> FormField {
    FormField {
        name: field.name.clone(),
        required: field.required,
        hint: field.hint.clone(),
        input: match &field.input {
            Input::Fixed(value) => FieldInput::Fixed {
                value: value.to_string(),
            },
            Input::Bool(_) => FieldInput::Bool,
            Input::Choice(choice) => FieldInput::Choice {
                options: choice
                    .options()
                    .iter()
                    .map(|option| serde_json::Value::String(option.clone()).to_string())
                    .collect(),
            },
            Input::Text(_) => FieldInput::Text,
            Input::Integer(_) => FieldInput::Integer,
            Input::Number(_) => FieldInput::Number,
            Input::List(_) => FieldInput::List,
            Input::Groups { fields, .. } => FieldInput::Groups {
                fields: fields.iter().map(form_field).collect(),
            },
        },
    }
}

/// The form of `verdict` for the request, with the values of `kept` (an earlier draft of this
/// verdict) when the recorded schema still takes them.
pub fn template(
    request: &Request,
    verdict: Verdict,
    kept: Option<Draft>,
) -> Result<FormView, Failure> {
    let form = match kept {
        Some(kept) if kept.verdict == verdict => filled(request, kept)?,
        _ => new(request, verdict)?,
    };
    Ok(project(verdict, &form))
}

pub(super) fn project(verdict: Verdict, form: &Form) -> FormView {
    FormView {
        verdict,
        fields: form.fields.iter().map(form_field).collect(),
        draft: draft_of(verdict, form),
    }
}

/// The review's form for `draft`: the recorded schema's fields with the draft's values.
fn filled(request: &Request, draft: Draft) -> Result<Form, Failure> {
    let verdict = draft.verdict.into();
    let fields = draft
        .fields
        .into_iter()
        .map(|(name, value)| (name, value.into()))
        .collect();
    let mut draft = review::Draft::default();
    match verdict {
        runtime::Verdict::Green => draft.green = fields,
        runtime::Verdict::Red => draft.red = fields,
    }
    draft.form(request, verdict).map_err(Failure::Action)
}

fn draft_of(verdict: Verdict, form: &Form) -> Draft {
    Draft {
        verdict,
        fields: review::values_of(&form.fields)
            .into_iter()
            .map(|(name, value)| (name, value.into()))
            .collect(),
    }
}

/// `draft` with field `name` set to `value`, as the library's form takes it.
pub fn edit_field(
    request: &Request,
    mut draft: Draft,
    name: &str,
    value: FieldValue,
) -> Result<Draft, Failure> {
    match draft.fields.iter_mut().find(|(field, _)| field == name) {
        Some((_, current)) => *current = value,
        None => draft.fields.push((name.to_owned(), value)),
    }
    let verdict = draft.verdict;
    Ok(draft_of(verdict, &filled(request, draft)?))
}

/// `draft` with an empty item added to the list field `name`: an empty text, or a group of
/// the list's fields as the library's form builds it.
pub fn add_item(request: &Request, draft: Draft, name: &str) -> Result<Draft, Failure> {
    edit_list(request, draft, name, |form, index| form.add(index))
}

/// `draft` with item `item` removed from the list field `name`; the library's form keeps one
/// empty item to type in.
pub fn remove_item(
    request: &Request,
    draft: Draft,
    name: &str,
    item: u32,
) -> Result<Draft, Failure> {
    edit_list(request, draft, name, |form, index| {
        form.remove(index, item as usize)
    })
}

fn edit_list(
    request: &Request,
    draft: Draft,
    name: &str,
    edit: impl FnOnce(&mut Form, usize),
) -> Result<Draft, Failure> {
    let verdict = draft.verdict;
    let mut form = filled(request, draft)?;
    let index = form
        .fields
        .iter()
        .position(|field| {
            field.name == name && matches!(field.input, Input::List(_) | Input::Groups { .. })
        })
        .ok_or_else(|| Failure::Missing(format!("{name} is not a list of this form.")))?;
    edit(&mut form, index);
    Ok(draft_of(verdict, &form))
}

/// The complete result of `draft`, or the form's own error for it.
pub fn result(request: &Request, draft: Draft) -> Result<serde_json::Value, Failure> {
    filled(request, draft)?.result().map_err(Failure::Invalid)
}
