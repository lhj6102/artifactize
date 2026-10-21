//! The review list's request search: a query of free text and `key:value` terms, matched
//! against saved requests, completed from the requests shown, and read page by page from the
//! state. Every review frontend (the terminal `review` and the desktop app) uses this one rule.
//!
//! Terms are ANDed. A plain word matches, case-insensitively, a substring of the repository,
//! eval, request ID or claimant. A `key:value` term restricts the match to one field:
//!
//! | Key | Matches |
//! |---|---|
//! | `repo:` | a substring of the repository path |
//! | `eval:` | a substring of the eval ID |
//! | `request:` | a substring of the request ID |
//! | `reviewer:` | a substring of the claimant; unclaimed requests never match |
//! | `tag:` | a substring of a tag of the eval's Artifact |
//! | `kind:` | the eval kind exactly: `runtime`, `agent` or `human` |
//! | `status:` | the request status; `waiting_human` includes `deferred` sign-offs |
//! | `is:` | `mine`, `claimed` or `unclaimed` (claims exist only on waiting Human requests) |
//!
//! Values with spaces are quoted: `repo:"/work/my repo"`. A quoted word is always free text, so
//! `"type:style"` searches for that text rather than naming a key.
#[cfg(test)]
mod tests;

use crate::{
    config::ProfileKind,
    store::{self, RequestView},
    types::{RequestStatus, ReviewerId},
};
use clap::ValueEnum;
use std::{collections::BTreeSet, ops::ControlFlow, path::Path};

/// The query the review list starts with: the waiting Human sign-offs.
pub const DEFAULT: &str = "kind:human status:waiting_human";

/// Requests read from the state per batch while a page is filled.
const BATCH: u32 = 400;
/// Matching requests per page, as the monitor pages its Runs.
pub const PAGE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Repo,
    Eval,
    Request,
    Reviewer,
    Tag,
    Kind,
    Status,
    Is,
}

impl Key {
    const ALL: [Self; 8] = [
        Self::Repo,
        Self::Eval,
        Self::Request,
        Self::Reviewer,
        Self::Tag,
        Self::Kind,
        Self::Status,
        Self::Is,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Eval => "eval",
            Self::Request => "request",
            Self::Reviewer => "reviewer",
            Self::Tag => "tag",
            Self::Kind => "kind",
            Self::Status => "status",
            Self::Is => "is",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|key| key.name().eq_ignore_ascii_case(name))
    }
}

/// A claim fact of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Is {
    Mine,
    Claimed,
    Unclaimed,
}

impl Is {
    const ALL: [Self; 3] = [Self::Mine, Self::Claimed, Self::Unclaimed];

    fn name(self) -> &'static str {
        match self {
            Self::Mine => "mine",
            Self::Claimed => "claimed",
            Self::Unclaimed => "unclaimed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Term {
    /// Lowercased text found in any listed column.
    Text(String),
    /// Lowercased text found in one field.
    Field(Key, String),
    Kind(ProfileKind),
    Status(RequestStatus),
    Is(Is),
}

/// A parsed query; every term must match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    terms: Vec<Term>,
}

/// The fields of one request that a query matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub repository: String,
    pub eval: String,
    pub request: String,
    pub reviewer: Option<String>,
    pub mine: bool,
    pub kind: ProfileKind,
    pub status: RequestStatus,
    pub tags: Vec<String>,
}

impl Entry {
    /// The searchable facts of `view` from repository `repo`, for the current `reviewer`.
    pub fn new(view: &RequestView, repo: &Path, reviewer: &ReviewerId) -> Self {
        let request = &view.request;
        let tags = view
            .definition
            .as_ref()
            .and_then(|definition| definition.artifact.tags.value())
            .or_else(|| {
                let artifacts = request.human_definition.as_ref()?.artifacts.value()?;
                artifacts.get(&request.target)?.tags.value()
            })
            .cloned()
            .unwrap_or_default();
        Self {
            repository: crate::platform::path_text(repo),
            eval: request.eval_id.to_string(),
            request: request.id.to_string(),
            reviewer: view.claim.as_ref().map(|claim| claim.reviewer.to_string()),
            mine: view
                .claim
                .as_ref()
                .is_some_and(|claim| &claim.reviewer == reviewer),
            kind: request.profile.kind(),
            status: request.status(),
            tags,
        }
    }
}

fn kind_name(kind: ProfileKind) -> &'static str {
    kind.name()
}

fn status_name(status: RequestStatus) -> String {
    status.as_str().to_ascii_lowercase()
}

/// One word of a query: where it starts in the text, its text without quotes, and whether it
/// began quoted.
struct Word {
    start: usize,
    text: String,
    quoted: bool,
}

/// Words of `text`, a double-quoted run counting as one word, and whether the text ends inside
/// a word (no whitespace after it outside quotes): the term still being typed.
fn scan(text: &str) -> (Vec<Word>, bool) {
    let mut words = Vec::new();
    let mut word: Option<Word> = None;
    let mut quoted = false;
    for (index, character) in text.char_indices() {
        if character.is_whitespace() && !quoted {
            words.extend(word.take());
            continue;
        }
        let current = word.get_or_insert_with(|| Word {
            start: index,
            text: String::new(),
            quoted: character == '"',
        });
        if character == '"' {
            quoted = !quoted;
        } else {
            current.text.push(character);
        }
    }
    let open = word.is_some();
    words.extend(word);
    (words, open)
}

/// Words of `text` and whether each began quoted.
pub(crate) fn words(text: &str) -> Vec<(String, bool)> {
    scan(text)
        .0
        .into_iter()
        .map(|word| (word.text, word.quoted))
        .collect()
}

impl std::str::FromStr for Query {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let terms = words(text)
            .into_iter()
            .map(|(word, quoted)| {
                let Some((name, value)) = word.split_once(':').filter(|_| !quoted) else {
                    return Ok(Term::Text(word.to_lowercase()));
                };
                let key = Key::parse(name).ok_or_else(|| {
                    format!(
                        "Unknown filter key `{name}:`; use {}, or quote the word to search for it.",
                        keys()
                    )
                })?;
                if value.is_empty() {
                    return Err(format!("`{}:` needs a value.", key.name()));
                }
                let lower = value.to_lowercase();
                match key {
                    Key::Kind => ProfileKind::value_variants()
                        .iter()
                        .copied()
                        .find(|kind| kind_name(*kind) == lower)
                        .map(Term::Kind)
                        .ok_or_else(|| {
                            format!("Unknown kind `{value}`; use {}.", list(kind_values()))
                        }),
                    Key::Status => RequestStatus::ALL
                        .iter()
                        .copied()
                        .find(|status| status_name(*status) == lower)
                        .map(Term::Status)
                        .ok_or_else(|| {
                            format!("Unknown status `{value}`; use {}.", list(status_values()))
                        }),
                    Key::Is => Is::ALL
                        .into_iter()
                        .find(|is| is.name() == lower)
                        .map(Term::Is)
                        .ok_or_else(|| {
                            format!(
                                "Unknown qualifier `is:{value}`; use {}.",
                                list(Is::ALL.iter().map(|is| format!("is:{}", is.name())))
                            )
                        }),
                    key => Ok(Term::Field(key, lower)),
                }
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { terms })
    }
}

fn list(items: impl IntoIterator<Item = String>) -> String {
    let items: Vec<String> = items.into_iter().collect();
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

fn keys() -> String {
    list(Key::ALL.iter().map(|key| format!("{}:", key.name())))
}

fn kind_values() -> impl Iterator<Item = String> {
    ProfileKind::value_variants()
        .iter()
        .map(|kind| kind_name(*kind).to_owned())
}

fn status_values() -> impl Iterator<Item = String> {
    RequestStatus::ALL.iter().map(|status| status_name(*status))
}

fn contains(field: &str, text: &str) -> bool {
    field.to_lowercase().contains(text)
}

impl Query {
    /// The query of `--repo PATH`: the default with that repository's term.
    pub fn text_for(repo: Option<&Path>) -> String {
        match repo {
            Some(repo) => format!(
                "{DEFAULT} {}",
                quote("repo", &crate::platform::path_text(repo))
            ),
            None => DEFAULT.to_owned(),
        }
    }

    /// Whether only waiting Human reviews can match, as with the default query.
    pub fn waiting_human(&self) -> bool {
        self.terms.contains(&Term::Kind(ProfileKind::Human))
            && self
                .terms
                .contains(&Term::Status(RequestStatus::WaitingHuman))
    }

    pub fn matches(&self, entry: &Entry) -> bool {
        self.terms.iter().all(|term| match term {
            Term::Text(text) => [
                Some(entry.repository.as_str()),
                Some(entry.eval.as_str()),
                Some(entry.request.as_str()),
                entry.reviewer.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|field| contains(field, text)),
            Term::Field(Key::Repo, text) => contains(&entry.repository, text),
            Term::Field(Key::Eval, text) => contains(&entry.eval, text),
            Term::Field(Key::Request, text) => contains(&entry.request, text),
            Term::Field(Key::Reviewer, text) => entry
                .reviewer
                .as_deref()
                .is_some_and(|reviewer| contains(reviewer, text)),
            Term::Field(Key::Tag, text) => entry.tags.iter().any(|tag| contains(tag, text)),
            Term::Field(_, _) => unreachable!("kind, status and is parse to their own terms"),
            Term::Kind(kind) => entry.kind == *kind,
            Term::Status(RequestStatus::WaitingHuman) => entry.status.is_human_wait(),
            Term::Status(status) => entry.status == *status,
            Term::Is(Is::Mine) => entry.mine,
            Term::Is(Is::Claimed) => entry.reviewer.is_some(),
            Term::Is(Is::Unclaimed) => entry.status.is_human_wait() && entry.reviewer.is_none(),
        })
    }

    /// The statuses every match must have, for the state read to select up front.
    fn statuses(&self) -> Vec<RequestStatus> {
        let statuses: BTreeSet<_> = self
            .terms
            .iter()
            .filter_map(|term| match term {
                Term::Status(status) => Some(*status),
                // Claims exist only on waiting Human requests.
                Term::Is(Is::Mine | Is::Claimed | Is::Unclaimed) => {
                    Some(RequestStatus::WaitingHuman)
                }
                _ => None,
            })
            .collect();
        match statuses.len() {
            // Two different required statuses match nothing; reading one keeps it bounded.
            0 | 1 => statuses
                .into_iter()
                .flat_map(|status| {
                    if status == RequestStatus::WaitingHuman {
                        vec![status, RequestStatus::Deferred]
                    } else {
                        vec![status]
                    }
                })
                .collect(),
            _ => vec![*statuses.first().expect("two statuses")],
        }
    }
}

/// `key:value`, quoted when the value has spaces or quotes.
pub(crate) fn quote(key: &str, value: &str) -> String {
    if value
        .chars()
        .any(|character| character.is_whitespace() || character == '"')
    {
        format!("{key}:\"{}\"", value.replace('"', ""))
    } else {
        format!("{key}:{value}")
    }
}

/// Completion of the query's last term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// The query before the completed term; a candidate replaces everything after it.
    pub keep: String,
    /// Whole terms, keys first (`repo:`) or `key:value` once a key is typed.
    pub candidates: Vec<String>,
}

/// `text` split before the term still being typed, as the parser reads words: a space inside
/// quotes (`repo:"my r`) stays in the term; after a finished word the term is empty.
pub(crate) fn last_term(text: &str) -> (&str, &str) {
    match scan(text) {
        (words, true) => text.split_at(words.last().map_or(0, |word| word.start)),
        (_, false) => (text, ""),
    }
}

/// The candidates for the query's last term: keys whose names start with it, or after a key
/// the values of that field among `entries` (fixed vocabularies for `kind:`, `status:`, `is:`).
pub fn complete(text: &str, entries: &[Entry]) -> Completion {
    let (keep, term) = last_term(text);
    // A quoted word is searched for literally: nothing to complete.
    let candidates = match term.split_once(':').filter(|_| !term.starts_with('"')) {
        None if term.starts_with('"') => Vec::new(),
        None => {
            let lower = term.to_lowercase();
            Key::ALL
                .iter()
                .map(|key| format!("{}:", key.name()))
                .filter(|key| key.starts_with(&lower))
                .collect()
        }
        Some((name, partial)) => match Key::parse(name) {
            None => Vec::new(),
            Some(key) => {
                let partial = partial.trim_matches('"').to_lowercase();
                let values: BTreeSet<String> = match key {
                    Key::Repo => entries
                        .iter()
                        .map(|entry| {
                            Path::new(&entry.repository)
                                .file_name()
                                .map_or(entry.repository.clone(), |name| {
                                    name.to_string_lossy().into_owned()
                                })
                        })
                        .collect(),
                    Key::Eval => entries.iter().map(|entry| entry.eval.clone()).collect(),
                    Key::Request => entries.iter().map(|entry| entry.request.clone()).collect(),
                    Key::Reviewer => entries
                        .iter()
                        .filter_map(|entry| entry.reviewer.clone())
                        .collect(),
                    Key::Tag => entries
                        .iter()
                        .flat_map(|entry| entry.tags.iter().cloned())
                        .collect(),
                    Key::Kind => kind_values().collect(),
                    Key::Status => status_values().collect(),
                    Key::Is => Is::ALL.iter().map(|is| is.name().to_owned()).collect(),
                };
                values
                    .into_iter()
                    .filter(|value| value.to_lowercase().contains(&partial))
                    .map(|value| quote(key.name(), &value))
                    .collect()
            }
        },
    };
    Completion {
        keep: keep.to_owned(),
        candidates,
    }
}

/// Complete the last term of `text` from the requests the terms before it find (their first
/// page): what the rest of the query lists, never the half-typed term itself.
pub async fn suggest(
    state: &Path,
    text: &str,
    reviewer: &ReviewerId,
) -> Result<Completion, String> {
    let (keep, term) = last_term(text);
    let from_requests = term
        .split_once(':')
        .filter(|_| !term.starts_with('"'))
        .and_then(|(name, _)| Key::parse(name))
        .is_some_and(|key| {
            matches!(
                key,
                Key::Repo | Key::Eval | Key::Request | Key::Reviewer | Key::Tag
            )
        });
    let entries = match keep.parse::<Query>() {
        Ok(query) if from_requests => search(state, &query, reviewer, 0)
            .await?
            .requests
            .into_iter()
            .map(|(_, entry)| entry)
            .collect(),
        _ => Vec::new(),
    };
    Ok(complete(text, &entries))
}

/// One page of matching requests, newest Run first, and where the next page starts.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub requests: Vec<(RequestView, Entry)>,
    /// The position of the next page in the state's request order; `None` on the last page.
    pub next: Option<u64>,
}

/// Up to [`PAGE`] requests matching `query` from position `start` on, read in bounded batches.
///
/// A waiting Human request that joined another waiting request's execution (a follower) is
/// listed once, as the original that records its definition and receives every action; a
/// follower whose original no longer waits is listed as itself.
pub async fn search(
    state: &Path,
    query: &Query,
    reviewer: &ReviewerId,
    start: u64,
) -> Result<Found, String> {
    let mut found = Found::default();
    read_matching(state, query, reviewer, start, |view, entry, position| {
        found.requests.push((view, entry));
        if found.requests.len() < PAGE {
            return ControlFlow::Continue(());
        }
        found.next = Some(position + 1);
        ControlFlow::Break(())
    })
    .await?;
    Ok(found)
}

/// How many requests `query` finds on all its pages together, each counted as [`search`]
/// lists it.
pub async fn count(state: &Path, query: &Query, reviewer: &ReviewerId) -> Result<u64, String> {
    let mut count = 0;
    read_matching(state, query, reviewer, 0, |_, _, _| {
        count += 1;
        ControlFlow::Continue(())
    })
    .await?;
    Ok(count)
}

/// Pass each request matching `query` from position `start` on to `found` with its position,
/// in the state's request order, until `found` breaks or the requests end.
async fn read_matching(
    state: &Path,
    query: &Query,
    reviewer: &ReviewerId,
    start: u64,
    mut found: impl FnMut(RequestView, Entry, u64) -> ControlFlow<()>,
) -> Result<(), String> {
    let statuses = query.statuses();
    let mut position = start;
    loop {
        let batch = store::read_request_batch(state, &statuses, position, BATCH).await?;
        let read = batch.len() as u64;
        for (offset, (view, repo)) in batch.into_iter().enumerate() {
            if Box::pin(follows_waiting(state, &view)).await {
                continue;
            }
            let entry = Entry::new(&view, &repo, reviewer);
            if query.matches(&entry) && found(view, entry, position + offset as u64).is_break() {
                return Ok(());
            }
        }
        if read < u64::from(BATCH) {
            return Ok(());
        }
        position += read;
    }
}

/// A waiting follower whose original still waits: the original's own row lists it.
async fn follows_waiting(state: &Path, view: &RequestView) -> bool {
    if !view.request.status().is_human_wait() {
        return false;
    }
    let Some(source) = view
        .execution
        .as_ref()
        .map(|execution| &execution.provenance.request_id)
        .filter(|source| **source != view.request.id)
    else {
        return false;
    };
    store::read_request(state, source)
        .await
        .is_ok_and(|original| original.request.status().is_human_wait())
}
