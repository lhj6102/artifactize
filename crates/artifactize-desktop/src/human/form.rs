//! GREEN/RED owner fields between the frontend's drafts and the review's [`Form`]: typed
//! fields for a flat schema, JSON text otherwise. Which schema gets which inputs, how values
//! become the result and the local errors all stay in [`artifactize_core::review::Form`].
use crate::Failure;
use artifactize_core::{
    review::{self, Form, Input},
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

/// How one owner field is entered.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FieldInput {
    /// A `const`: prefilled and fixed; `value` is its JSON.
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

/// A new form: typed `fields`, or `json` text to edit when the schema is not flat.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FormView {
    pub verdict: Verdict,
    pub fields: Vec<FormField>,
    pub json: Option<String>,
}

/// A field's draft value, by the field's kind.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum FieldValue {
    Fixed,
    Bool(Option<bool>),
    /// The chosen option's index.
    Choice(Option<u32>),
    Text(String),
    Integer(String),
    Number(String),
}

/// The frontend's draft of one verdict: field values by name, or JSON text.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub verdict: Verdict,
    pub fields: Vec<(String, FieldValue)>,
    pub json: Option<String>,
}

fn new(request: &Request, verdict: Verdict) -> Result<Form, Failure> {
    let schema = review::schema(request, verdict.into()).map_err(Failure::Action)?;
    Ok(Form::new(verdict.into(), schema.as_ref()))
}

pub fn template(request: &Request, verdict: Verdict) -> Result<FormView, Failure> {
    let form = new(request, verdict)?;
    let fields = form
        .fields
        .iter()
        .map(|field| FormField {
            name: field.name.clone(),
            required: field.required,
            hint: field.hint.clone(),
            input: match &field.input {
                Input::Fixed(value) => FieldInput::Fixed {
                    value: value.to_string(),
                },
                Input::Bool(_) => FieldInput::Bool,
                Input::Choice(options, _) => FieldInput::Choice {
                    options: options.iter().map(ToString::to_string).collect(),
                },
                Input::Text(_) => FieldInput::Text,
                Input::Integer(_) => FieldInput::Integer,
                Input::Number(_) => FieldInput::Number,
            },
        })
        .collect();
    Ok(FormView {
        verdict,
        fields,
        json: form.json,
    })
}

/// The review's form for `draft`: the recorded schema's fields with the draft's values, or its
/// JSON text.
fn filled(request: &Request, draft: Draft) -> Result<Form, Failure> {
    let mut form = new(request, draft.verdict)?;
    match draft.json {
        Some(json) => form.set_json(json),
        None => {
            for field in &mut form.fields {
                let Some((_, value)) = draft.fields.iter().find(|(name, _)| *name == field.name)
                else {
                    continue;
                };
                field.input = match (&field.input, value) {
                    (Input::Bool(_), FieldValue::Bool(value)) => Input::Bool(*value),
                    (Input::Choice(options, _), FieldValue::Choice(index)) => Input::Choice(
                        options.clone(),
                        index
                            .map(|index| index as usize)
                            .filter(|index| *index < options.len()),
                    ),
                    (Input::Text(_), FieldValue::Text(text)) => Input::Text(text.clone()),
                    (Input::Integer(_), FieldValue::Integer(text)) => Input::Integer(text.clone()),
                    (Input::Number(_), FieldValue::Number(text)) => Input::Number(text.clone()),
                    // A constant stays fixed; a value of another kind is not this field's.
                    (input, _) => input.clone(),
                };
            }
        }
    }
    Ok(form)
}

/// The complete result of `draft`, or the form's own error for it.
pub fn result(request: &Request, draft: Draft) -> Result<serde_json::Value, Failure> {
    filled(request, draft)?.result().map_err(Failure::Invalid)
}

/// The owner-fields JSON of `draft`, as the terminal review opens a form in `$EDITOR`.
pub fn json(request: &Request, draft: Draft) -> Result<String, Failure> {
    Ok(filled(request, draft)?.draft())
}
