//! Where a review's reasons go in each verdict's answer, and the sentence that says why a
//! verdict cannot be submitted yet. The recorded schemas decide both: RED puts the reasons
//! into its single list of strings, else joins them into its text field; GREEN joins them
//! into its text field.

use jsonschema::error::ValidationErrorKind;
use serde_json::{Map, Value};

use super::{Form, Input, Submission};
use crate::runtime::Verdict;

/// Reasons joined into one text field are separate paragraphs.
const PARAGRAPH: &str = "\n\n";
/// The text field that takes the reasons when a schema has several.
const COMMENT: &str = "comment";

/// What a field holds, as far as reasons are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    List,
    Text,
    Other,
}

impl From<&Input> for Shape {
    fn from(input: &Input) -> Self {
        match input {
            Input::List(_) => Self::List,
            Input::Text(_) => Self::Text,
            _ => Self::Other,
        }
    }
}

/// How the reasons fill their field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Slot {
    /// One list item per reason.
    Items,
    /// The reasons joined with blank lines.
    Joined,
}

/// The field (its position among `fields`) that takes the reasons of `verdict`, if any.
pub(super) fn target<'a>(
    verdict: Verdict,
    fields: impl IntoIterator<Item = (&'a str, Shape)>,
) -> Option<(usize, Slot)> {
    let fields: Vec<_> = fields.into_iter().collect();
    let of = |shape: Shape| {
        fields
            .iter()
            .enumerate()
            .filter(move |(_, (_, kind))| *kind == shape)
            .map(|(index, (name, _))| (index, *name))
            .collect::<Vec<_>>()
    };
    if verdict == Verdict::Red
        && let [(list, _)] = of(Shape::List)[..]
    {
        return Some((list, Slot::Items));
    }
    let texts = of(Shape::Text);
    let comment = texts.iter().find(|(_, name)| *name == COMMENT);
    let text = comment.or(match &texts[..] {
        [only] => Some(only),
        _ => None,
    });
    text.map(|(index, _)| (*index, Slot::Joined))
}

fn target_of(form: &Form) -> Option<(usize, Slot)> {
    target(
        form.verdict,
        form.fields
            .iter()
            .map(|field| (field.name.as_str(), Shape::from(&field.input))),
    )
}

/// The name of the field of `form` the reasons fill.
pub(super) fn target_name(form: &Form) -> Option<&str> {
    target_of(form).map(|(index, _)| form.fields[index].name.as_str())
}

/// The owner fields a reviewer edits: `form` without the field the reasons fill.
pub(super) fn hide(form: &mut Form) {
    if let Some((index, _)) = target_of(form) {
        form.fields.remove(index);
    }
}

/// The reasons an earlier draft entered in a field: a list's items, or its one text.
pub(super) fn entered(items: &[String]) -> Vec<String> {
    items
        .iter()
        .filter(|item| !item.trim().is_empty())
        .cloned()
        .collect()
}

/// The answer `reasons` and the owner fields of `form` make, or why it cannot be submitted.
/// `form` holds every recorded field; `schema` is the verdict's recorded owner schema.
pub(super) fn answer(
    mut form: Form,
    reasons: &[String],
    schema: Option<&Map<String, Value>>,
) -> Result<Submission, String> {
    let verdict = form.verdict;
    let target = target_of(&form);
    let order: Vec<String> = form.fields.iter().map(|field| field.name.clone()).collect();
    let name = match target {
        Some((index, slot)) => {
            let field = &mut form.fields[index];
            field.input = match slot {
                Slot::Items => Input::List(reasons.to_vec()),
                Slot::Joined => Input::Text(reasons.join(PARAGRAPH)),
            };
            Some(field.name.clone())
        }
        None if reasons.is_empty() => None,
        None => return Err(format!("{verdict} takes no reasons; remove them.")),
    };
    let submission = form.submission()?;
    let Some(schema) = schema else {
        return Ok(submission);
    };
    // The owner schema describes the owner fields; the verdict is the envelope's.
    let mut fields = submission.to_json();
    fields
        .as_object_mut()
        .expect("a submission is an object")
        .remove("verdict");
    let refused = Refused {
        verdict,
        target: name.as_deref(),
        reasons: reasons.len(),
        order: &order,
    };
    match refused.first(schema, &fields) {
        Some(sentence) => Err(sentence),
        None => Ok(submission),
    }
}

/// What a refusal sentence needs to know about the answer it explains.
struct Refused<'a> {
    verdict: Verdict,
    /// The field the reasons fill.
    target: Option<&'a str>,
    reasons: usize,
    /// The owner fields in form order, which orders the refusals after the reasons'.
    order: &'a [String],
}

impl Refused<'_> {
    /// The refusal a reviewer acts on first: the reasons', then by form order.
    fn first(&self, schema: &Map<String, Value>, fields: &Value) -> Option<String> {
        let Ok(validator) = jsonschema::validator_for(&Value::Object(schema.clone())) else {
            return Some(format!(
                "The recorded {} schema is not valid JSON Schema.",
                self.verdict
            ));
        };
        validator
            .iter_errors(fields)
            .map(|error| {
                let mut path: Vec<String> = error
                    .instance_path()
                    .segments()
                    .map(|segment| segment.to_string())
                    .collect();
                if let ValidationErrorKind::Required { property } = error.kind() {
                    path.push(property.as_str().unwrap_or_default().to_owned());
                }
                let rank = match path.first() {
                    Some(field) if Some(field.as_str()) == self.target => 0,
                    Some(field) => {
                        1 + self
                            .order
                            .iter()
                            .position(|name| name == field)
                            .unwrap_or(self.order.len())
                    }
                    None => usize::MAX,
                };
                (
                    rank,
                    self.sentence(error.kind(), &path, error.instance(), &error.to_string()),
                )
            })
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, sentence)| sentence)
    }

    fn sentence(
        &self,
        kind: &ValidationErrorKind,
        path: &[String],
        instance: &Value,
        detail: &str,
    ) -> String {
        let verdict = self.verdict;
        let none = self.reasons == 0;
        if path.first().map(String::as_str) == self.target && self.target.is_some() {
            let needs = |limit: u64| match limit {
                1 => format!("{verdict} needs a reason."),
                limit => format!("{verdict} needs at least {limit} reasons."),
            };
            return match (kind, path) {
                (ValidationErrorKind::Required { .. }, _) => needs(1),
                (ValidationErrorKind::MinItems { limit }, _) => needs(*limit),
                (ValidationErrorKind::MaxItems { limit }, _) => {
                    format!("{verdict} takes at most {limit} reasons.")
                }
                (ValidationErrorKind::MinLength { .. }, [_]) if none => needs(1),
                (ValidationErrorKind::MinLength { limit }, [_]) => {
                    format!("{verdict}'s reasons need at least {limit} characters.")
                }
                (ValidationErrorKind::MaxLength { limit }, [_]) => {
                    format!("{verdict}'s reasons take at most {limit} characters.")
                }
                (ValidationErrorKind::MinLength { limit }, [_, item]) => {
                    format!(
                        "Reason {} needs at least {limit} characters.",
                        ordinal(item)
                    )
                }
                (ValidationErrorKind::MaxLength { limit }, [_, item]) => {
                    format!("Reason {} takes at most {limit} characters.", ordinal(item))
                }
                _ => format!("{verdict}'s reasons: {detail}."),
            };
        }
        let label = label(path);
        let empty = instance.as_str().is_some_and(|text| text.trim().is_empty());
        match kind {
            ValidationErrorKind::Required { .. } => format!("Fill in {label}."),
            ValidationErrorKind::MinLength { .. } if empty => format!("Fill in {label}."),
            ValidationErrorKind::MinLength { limit } => {
                format!("{label} needs at least {limit} characters.")
            }
            ValidationErrorKind::MaxLength { limit } => {
                format!("{label} takes at most {limit} characters.")
            }
            ValidationErrorKind::MinItems { limit: 1 } => format!("Add an item to {label}."),
            ValidationErrorKind::MinItems { limit } => {
                format!("{label} needs at least {limit} items.")
            }
            ValidationErrorKind::MaxItems { limit } => {
                format!("{label} takes at most {limit} items.")
            }
            ValidationErrorKind::Minimum { limit } => format!("{label} must be at least {limit}."),
            ValidationErrorKind::ExclusiveMinimum { limit } => {
                format!("{label} must be more than {limit}.")
            }
            ValidationErrorKind::Maximum { limit } => format!("{label} must be at most {limit}."),
            ValidationErrorKind::ExclusiveMaximum { limit } => {
                format!("{label} must be less than {limit}.")
            }
            ValidationErrorKind::MultipleOf { multiple_of, .. } => {
                format!("{label} must be a multiple of {multiple_of}.")
            }
            _ => format!("{label}: {detail}."),
        }
    }
}

/// A field as the form names it: `steps 2` for a list item, `checks 1 · name` in a group.
fn label(path: &[String]) -> String {
    match path {
        [] => "the answer".into(),
        [field] => field.clone(),
        [field, item] => format!("{field} {}", ordinal(item)),
        [field, item, sub, ..] => format!("{field} {} · {sub}", ordinal(item)),
    }
}

/// A list position counted from one, as a reviewer sees it.
fn ordinal(index: &str) -> String {
    index
        .parse::<usize>()
        .map_or_else(|_| index.to_owned(), |index| (index + 1).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn form(verdict: Verdict, schema: &Value) -> Form {
        Form::new(verdict, Some(schema)).unwrap()
    }

    fn owner(schema: &Value) -> &Map<String, Value> {
        schema.as_object().unwrap()
    }

    fn reasons(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_owned()).collect()
    }

    fn set(form: &mut Form, name: &str, input: Input) {
        form.fields
            .iter_mut()
            .find(|field| field.name == name)
            .unwrap()
            .input = input;
    }

    #[test]
    fn red_reasons_become_the_items_of_its_one_list_and_min_items_asks_for_one() {
        let schema = json!({
            "type": "object",
            "properties": {
                "change": {"type": "array", "items": {"type": "string"}, "minItems": 1},
                "severity": {"enum": ["minor", "major"]},
                "kind": {"const": "review"},
            },
            "required": ["change", "severity", "kind"],
        });
        let mut hidden = form(Verdict::Red, &schema);
        hide(&mut hidden);
        let names: Vec<_> = hidden
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect();
        assert_eq!(names, ["kind", "severity"]);

        let empty = answer(form(Verdict::Red, &schema), &[], Some(owner(&schema)));
        assert_eq!(empty.unwrap_err(), "RED needs a reason.");
        let unrated = answer(
            form(Verdict::Red, &schema),
            &reasons(&["Totals are wrong"]),
            Some(owner(&schema)),
        );
        assert_eq!(unrated.unwrap_err(), "Fill in severity.");

        let mut rated = form(Verdict::Red, &schema);
        let severity = rated.fields.iter().find(|field| field.name == "severity");
        let Some(Input::Choice(mut choice)) = severity.map(|field| field.input.clone()) else {
            panic!("severity is a choice");
        };
        choice.select(Some(1));
        set(&mut rated, "severity", Input::Choice(choice));
        let submitted = answer(
            rated,
            &reasons(&["Totals are wrong", "The footer overlaps"]),
            Some(owner(&schema)),
        )
        .unwrap();
        assert_eq!(
            submitted.to_json(),
            json!({
                "verdict": "RED",
                "change": ["Totals are wrong", "The footer overlaps"],
                "severity": "major",
                "kind": "review",
            })
        );
    }

    #[test]
    fn without_a_list_red_joins_its_reasons_into_the_text_field() {
        let schema = json!({
            "type": "object",
            "properties": {"reason": {"type": "string", "minLength": 3}},
            "required": ["reason"],
        });
        assert_eq!(
            answer(form(Verdict::Red, &schema), &[], Some(owner(&schema))).unwrap_err(),
            "RED needs a reason."
        );
        assert_eq!(
            answer(
                form(Verdict::Red, &schema),
                &reasons(&["no"]),
                Some(owner(&schema))
            )
            .unwrap_err(),
            "RED's reasons need at least 3 characters."
        );
        let submitted = answer(
            form(Verdict::Red, &schema),
            &reasons(&["First", "Second"]),
            Some(owner(&schema)),
        )
        .unwrap();
        assert_eq!(
            submitted.to_json(),
            json!({"verdict": "RED", "reason": "First\n\nSecond"})
        );
    }

    #[test]
    fn a_schema_without_a_place_for_reasons_blocks_only_while_reasons_exist() {
        let red = json!({
            "type": "object",
            "properties": {"severity": {"enum": ["minor"]}},
            "required": ["severity"],
        });
        assert_eq!(
            answer(
                form(Verdict::Red, &red),
                &reasons(&["Why"]),
                Some(owner(&red))
            )
            .unwrap_err(),
            "RED takes no reasons; remove them."
        );
        assert_eq!(
            answer(form(Verdict::Red, &red), &[], Some(owner(&red))).unwrap_err(),
            "Fill in severity."
        );
        let green = json!({
            "type": "object",
            "properties": {"approved": {"const": true}},
            "required": ["approved"],
        });
        assert_eq!(
            answer(
                form(Verdict::Green, &green),
                &reasons(&["Looks right"]),
                Some(owner(&green))
            )
            .unwrap_err(),
            "GREEN takes no reasons; remove them."
        );
        assert_eq!(
            answer(form(Verdict::Green, &green), &[], Some(owner(&green)))
                .unwrap()
                .to_json(),
            json!({"verdict": "GREEN", "approved": true})
        );
    }

    #[test]
    fn green_prefers_comment_among_several_text_fields_and_otherwise_needs_exactly_one() {
        let schema = json!({
            "type": "object",
            "properties": {
                "summary": {"type": "string"},
                "comment": {"type": "string", "minLength": 1},
            },
            "required": ["summary", "comment"],
        });
        let mut hidden = form(Verdict::Green, &schema);
        hide(&mut hidden);
        assert_eq!(hidden.fields[0].name, "summary");
        assert_eq!(
            answer(form(Verdict::Green, &schema), &[], Some(owner(&schema))).unwrap_err(),
            "GREEN needs a reason."
        );
        let submitted = answer(
            form(Verdict::Green, &schema),
            &reasons(&["Checked the totals", "Checked the footer"]),
            Some(owner(&schema)),
        )
        .unwrap();
        assert_eq!(
            submitted.to_json()["comment"],
            "Checked the totals\n\nChecked the footer"
        );

        let ambiguous = json!({
            "type": "object",
            "properties": {"left": {"type": "string"}, "right": {"type": "string"}},
        });
        assert_eq!(
            answer(
                form(Verdict::Green, &ambiguous),
                &reasons(&["Why"]),
                Some(owner(&ambiguous))
            )
            .unwrap_err(),
            "GREEN takes no reasons; remove them."
        );
    }

    #[test]
    fn red_with_two_lists_uses_its_text_field_and_names_owner_field_limits() {
        let schema = json!({
            "type": "object",
            "properties": {
                "comment": {"type": "string"},
                "steps": {"type": "array", "items": {"type": "string"}, "minItems": 2},
                "files": {"type": "array", "items": {"type": "string"}},
                "count": {"type": "integer", "maximum": 3},
            },
            "required": ["steps"],
        });
        let mut hidden = form(Verdict::Red, &schema);
        hide(&mut hidden);
        assert!(hidden.fields.iter().all(|field| field.name != "comment"));
        let mut entered = form(Verdict::Red, &schema);
        set(&mut entered, "steps", Input::List(reasons(&["one", "two"])));
        set(&mut entered, "count", Input::Integer("4".into()));
        assert_eq!(
            answer(entered, &reasons(&["Why"]), Some(owner(&schema))).unwrap_err(),
            "count must be at most 3."
        );
        assert_eq!(
            answer(form(Verdict::Red, &schema), &[], Some(owner(&schema))).unwrap_err(),
            "steps needs at least 2 items."
        );
    }
}
