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

/// The frontend's draft of one verdict: field values by name.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub verdict: Verdict,
    pub fields: Vec<(String, FieldValue)>,
}

fn new(request: &Request, verdict: Verdict) -> Result<Form, Failure> {
    let schema = review::schema(request, verdict.into()).map_err(Failure::Action)?;
    Form::new(verdict.into(), schema.as_ref()).map_err(Failure::Action)
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
            Input::Choice(options, _) => FieldInput::Choice {
                options: options
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
    Ok(FormView {
        verdict,
        fields: form.fields.iter().map(form_field).collect(),
        draft: draft_of(verdict, &form),
    })
}

/// The review's form for `draft`: the recorded schema's fields with the draft's values.
fn filled(request: &Request, draft: Draft) -> Result<Form, Failure> {
    let mut form = new(request, draft.verdict)?;
    fill(&mut form.fields, &draft.fields);
    Ok(form)
}

/// `fields` holding the draft `values` their kinds take, by name.
fn fill(fields: &mut [Field], values: &[(String, FieldValue)]) {
    for field in fields {
        let Some((_, value)) = values.iter().find(|(name, _)| *name == field.name) else {
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
            (Input::List(_), FieldValue::List(items)) => Input::List(items.clone()),
            (Input::Groups { fields, .. }, FieldValue::Groups(values)) => Input::Groups {
                fields: fields.clone(),
                items: values
                    .iter()
                    .map(|values| {
                        let mut item = fields.clone();
                        fill(&mut item, values);
                        item
                    })
                    .collect(),
            },
            // A constant stays fixed; a value of another kind is not this field's.
            (input, _) => input.clone(),
        };
    }
}

/// A field's draft value as the form holds it.
fn value_of(input: &Input) -> FieldValue {
    match input {
        Input::Fixed(_) => FieldValue::Fixed,
        Input::Bool(value) => FieldValue::Bool(*value),
        Input::Choice(_, index) => FieldValue::Choice(index.map(|index| index as u32)),
        Input::Text(text) => FieldValue::Text(text.clone()),
        Input::Integer(text) => FieldValue::Integer(text.clone()),
        Input::Number(text) => FieldValue::Number(text.clone()),
        Input::List(items) => FieldValue::List(items.clone()),
        Input::Groups { items, .. } => {
            FieldValue::Groups(items.iter().map(|item| values_of(item)).collect())
        }
    }
}

fn values_of(fields: &[Field]) -> Vec<(String, FieldValue)> {
    fields
        .iter()
        .map(|field| (field.name.clone(), value_of(&field.input)))
        .collect()
}

/// The draft a form holds: each field's value.
fn draft_of(verdict: Verdict, form: &Form) -> Draft {
    Draft {
        verdict,
        fields: values_of(&form.fields),
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
