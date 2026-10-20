//! A saved Run record read in two parts. Its definitions are fixed for the Run's life and can
//! be by far the largest part of it (hundreds of megabytes for a catalog of Artifacts), while
//! the rest changes as the Run goes on: every execution it starts saves the record again. A
//! reader keeps what it learned about definitions it has seen, by their digest, and decodes only
//! the rest of a record whose definitions it has seen before.
use std::collections::{BTreeMap, BTreeSet};

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;

use super::{Run, Unreadable, definitions::Definitions, unreadable::decode};
use crate::types::Sha256Bytes;

/// The member of a Run record that holds its definitions.
const DEFINITIONS: &str = "definitions";

/// What a reader learned about the definitions of the Runs it read.
#[derive(Default)]
pub(crate) struct Seen {
    /// Whether the definitions with each digest decode: `None` when they do, else the reason.
    verdicts: BTreeMap<Sha256Bytes, Option<String>>,
    /// The definitions [`Seen::read`] decoded last, for the Run the reader shows.
    kept: Option<(Sha256Bytes, Definitions)>,
}

/// One saved record's members in order, each still as its JSON text.
struct Members<'a>(Vec<(String, &'a RawValue)>);

impl<'de> Deserialize<'de> for Members<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = Members<'de>;
            fn expecting(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                out.write_str("a Run record")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut members = Vec::new();
                while let Some(member) = map.next_entry::<String, &'de RawValue>()? {
                    members.push(member);
                }
                Ok(Members(members))
            }
        }
        deserializer.deserialize_map(Object)
    }
}

/// A record's definitions as saved: their JSON text and its digest.
type Saved<'a> = Option<(&'a str, Sha256Bytes)>;

/// The record's text without its definitions, and the definitions' text with its digest. A
/// record that names its definitions more than once keeps them all in the rest, which then
/// fails to decode just as the whole record would.
fn split(data: &str) -> Result<(String, Saved<'_>), serde_json::Error> {
    let Members(members) = serde_json::from_str(data)?;
    let single = members.iter().filter(|(key, _)| key == DEFINITIONS).count() == 1;
    let mut rest = String::with_capacity(data.len().min(1 << 20));
    let mut definitions = None;
    rest.push('{');
    for (key, value) in members {
        if single && key == DEFINITIONS {
            let text = value.get();
            definitions = Some((text, Sha256Bytes::of(text.as_bytes())));
            continue;
        }
        if rest.len() > 1 {
            rest.push(',');
        }
        rest.push_str(&serde_json::to_string(&key)?);
        rest.push(':');
        rest.push_str(value.get());
    }
    rest.push('}');
    Ok((rest, definitions))
}

impl Seen {
    /// The Run `id` saved as `data` without its definitions, which are neither decoded nor
    /// checked, and their digest: for readers of a Run's own facts (its status, repository and
    /// times), such as the catalog of repositories.
    pub(crate) fn facts(
        &self,
        id: &str,
        data: &str,
    ) -> Result<(Run, Option<Sha256Bytes>), Unreadable> {
        let (run, definitions) = rest(id, data)?;
        Ok((run, definitions.map(|(_, digest)| digest)))
    }

    /// Whether the Run `id` saved as `data` reads whole. Definitions seen before are not
    /// decoded again.
    pub(crate) fn check(&mut self, id: &str, data: &str) -> Result<(), Unreadable> {
        let (_, definitions) = rest(id, data)?;
        let Some((text, digest)) = definitions else {
            return Ok(());
        };
        let verdict = match self.verdicts.get(&digest) {
            Some(verdict) => verdict.clone(),
            None => {
                let verdict = decode::<Definitions>("run", id, text)
                    .err()
                    .map(|unreadable| unreadable.reason);
                self.verdicts.insert(digest, verdict.clone());
                verdict
            }
        };
        verdict.map_or(Ok(()), |reason| Err(unreadable(id, reason)))
    }

    /// The whole Run `id` saved as `data`. Its definitions are the ones decoded last when they
    /// have the same digest, shared rather than decoded again; otherwise they are decoded and
    /// kept for the next read.
    pub(crate) fn read(&mut self, id: &str, data: &str) -> Result<Run, Unreadable> {
        let (mut run, definitions) = rest(id, data)?;
        if let Some((text, digest)) = definitions {
            run.definitions = match &self.kept {
                Some((kept, definitions)) if *kept == digest => definitions.clone(),
                _ => match decode::<Definitions>("run", id, text) {
                    Ok(definitions) => {
                        self.verdicts.insert(digest, None);
                        self.kept = Some((digest, definitions.clone()));
                        definitions
                    }
                    Err(unreadable) => {
                        self.verdicts
                            .insert(digest, Some(unreadable.reason.clone()));
                        return Err(unreadable);
                    }
                },
            };
        }
        Ok(run)
    }

    /// Forget the verdicts of definitions no saved Run holds any more.
    pub(crate) fn retain(&mut self, held: &BTreeSet<Sha256Bytes>) {
        self.verdicts.retain(|digest, _| held.contains(digest));
    }
}

/// The record without its definitions, decoded, and its definitions' text and digest.
fn rest<'a>(id: &str, data: &'a str) -> Result<(Run, Saved<'a>), Unreadable> {
    let (rest, definitions) = split(data).map_err(|error| unreadable(id, error.to_string()))?;
    Ok((decode::<Run>("run", id, &rest)?, definitions))
}

fn unreadable(id: &str, reason: String) -> Unreadable {
    Unreadable {
        kind: "run".into(),
        id: id.to_owned(),
        reason,
    }
}

#[cfg(test)]
mod tests;
