//! The JSON schemas one read of declarations has met. Declarations often repeat one schema
//! thousands of times, such as an Agent tool's `input_schema` or an eval's verdict schemas in
//! generated reviews: each distinct schema is kept once and checked once.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};

/// The schemas of one read, by their JSON text, and what checking each found.
#[derive(Default)]
pub(crate) struct Schemas {
    tools: HashMap<String, Arc<Value>>,
    verdicts: HashMap<String, Arc<Map<String, Value>>>,
    /// Each kept schema by its address, which stays its own while this read keeps it, with
    /// what checking it found once checked.
    checked: HashMap<usize, Option<Result<(), String>>>,
}

impl Schemas {
    /// The schema equal to `schema` that this read keeps.
    pub(super) fn tool(&mut self, schema: &Arc<Value>) -> Arc<Value> {
        let kept = shared(&mut self.tools, schema);
        self.checked.entry(address(&kept)).or_default();
        kept
    }

    /// The verdict schema equal to `schema` that this read keeps.
    pub(super) fn verdict(&mut self, schema: &Arc<Map<String, Value>>) -> Arc<Map<String, Value>> {
        let kept = shared(&mut self.verdicts, schema);
        self.checked.entry(address(&kept)).or_default();
        kept
    }

    /// `check`'s result for `schema`: once for a schema this read keeps, however often it is
    /// declared, and every time for any other.
    pub(super) fn check<T>(
        &mut self,
        schema: &Arc<T>,
        check: impl FnOnce(&T) -> Result<(), String>,
    ) -> Result<(), String> {
        match self.checked.get_mut(&address(schema)) {
            Some(checked) => checked.get_or_insert_with(|| check(schema)).clone(),
            None => check(schema),
        }
    }
}

fn address<T>(schema: &Arc<T>) -> usize {
    Arc::as_ptr(schema).cast::<()>() as usize
}

fn shared<T: serde::Serialize>(kept: &mut HashMap<String, Arc<T>>, schema: &Arc<T>) -> Arc<T> {
    let text = serde_json::to_string(&**schema).expect("a schema is JSON");
    kept.entry(text).or_insert_with(|| schema.clone()).clone()
}
