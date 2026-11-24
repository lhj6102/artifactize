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
//! | `kind:`, `type:` | the eval kind exactly: `runtime`, `agent` or `human` |
//! | `status:` | the request status; `waiting_human` includes `deferred` sign-offs |
//! | `is:` | `mine`, `claimed` or `unclaimed` (claims exist only on waiting Human requests) |
//!
//! The bare term `waiting` matches unclaimed waiting Human requests, including deferred
//! sign-offs. It is the short spelling of `is:unclaimed`.
//!
//! Values with spaces are quoted: `repo:"/work/my repo"`. A quoted word is always free text, so
//! `"type:style"` searches for that text rather than naming a key.
#[cfg(test)]
mod tests;

use crate::{
    config::ProfileKind,
    monitor::live::{Label, Labels},
    store::{self, RequestView},
    types::{RequestId, RequestStatus, ReviewerId},
};
use clap::ValueEnum;
use std::{
    collections::BTreeSet,
    ops::ControlFlow,
    path::{Path, PathBuf},
};

/// The query the review list starts with: the waiting Human sign-offs.
pub const DEFAULT: &str = "type:human waiting";
const WAITING: &str = "waiting";

/// Requests read from the state per batch while a page is filled.
const BATCH: u32 = 400;
/// Matching requests per page, as the monitor pages its Runs.
pub const PAGE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextField {
    Repo,
    Branch,
    Eval,
    Request,
    Reviewer,
    Tag,
}

impl TextField {
    fn name(self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Branch => "branch",
            Self::Eval => "eval",
            Self::Request => "request",
            Self::Reviewer => "reviewer",
            Self::Tag => "tag",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Text(TextField),
    Kind,
    Type,
    Status,
    Is,
}

impl Key {
    const ALL: [Self; 10] = [
        Self::Text(TextField::Repo),
        Self::Text(TextField::Branch),
        Self::Text(TextField::Eval),
        Self::Text(TextField::Request),
        Self::Text(TextField::Reviewer),
        Self::Text(TextField::Tag),
        Self::Kind,
        Self::Type,
        Self::Status,
        Self::Is,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Text(field) => field.name(),
            Self::Kind => "kind",
            Self::Type => "type",
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
    Field(TextField, String),
    Kind(ProfileKind),
    Status(RequestStatus),
    Is(Is),
}

/// A parsed query; every term must match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    terms: Vec<Term>,
    places: bool,
    live: bool,
}

/// Who holds a request's claim, as the current reviewer sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    Unclaimed,
    /// Claimed by the current reviewer, named.
    Mine(String),
    /// Claimed by another reviewer, named.
    Other(String),
}

impl Claim {
    /// The claimant's name, whoever it is.
    pub fn reviewer(&self) -> Option<&str> {
        match self {
            Self::Unclaimed => None,
            Self::Mine(name) | Self::Other(name) => Some(name),
        }
    }
}

/// The fields of one request that a query matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub repository: PathBuf,
    pub label: Option<Label>,
    pub eval: String,
    pub request: String,
    pub claim: Claim,
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
            .and_then(|definition| definition.artifact().tags.value())
            .or_else(|| {
                let artifacts = request.human_definition.as_ref()?.artifacts.value()?;
                artifacts.get(&request.target)?.tags.value()
            })
            .cloned()
            .unwrap_or_default();
        Self {
            repository: repo.to_owned(),
            label: None,
            eval: request.eval_id.to_string(),
            request: request.id.to_string(),
            claim: match &view.claim {
                None => Claim::Unclaimed,
                Some(claim) if &claim.reviewer == reviewer => {
                    Claim::Mine(claim.reviewer.to_string())
                }
                Some(claim) => Claim::Other(claim.reviewer.to_string()),
            },
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
                if !quoted && word.eq_ignore_ascii_case(WAITING) {
                    return Ok(Term::Is(Is::Unclaimed));
                }
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
                    Key::Kind | Key::Type => ProfileKind::value_variants()
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
                    Key::Text(field) => Ok(Term::Field(field, lower)),
                }
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            terms,
            places: false,
            live: false,
        })
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
    /// Search the displayed projects and branches, instead of raw checkout paths.
    pub fn for_places(mut self) -> Self {
        self.places = true;
        self
    }

    /// Hide requests whose workspace is gone, without hiding the record from CLI history.
    pub fn for_live_repositories(mut self) -> Self {
        self.live = true;
        self
    }

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
            && self.terms.iter().any(|term| {
                matches!(
                    term,
                    Term::Status(RequestStatus::WaitingHuman | RequestStatus::Deferred)
                        | Term::Is(_)
                )
            })
    }

    pub fn matches(&self, entry: &Entry) -> bool {
        let repository = entry.label.as_ref().map_or_else(
            || crate::platform::path_text(&entry.repository),
            |label| label.project.clone(),
        );
        self.terms.iter().all(|term| match term {
            Term::Text(text) => [
                Some(repository.as_str()),
                entry.label.as_ref().map(|label| label.branch.as_str()),
                Some(entry.eval.as_str()),
                Some(entry.request.as_str()),
                entry.claim.reviewer(),
            ]
            .into_iter()
            .flatten()
            .any(|field| contains(field, text)),
            Term::Field(TextField::Repo, text) => contains(&repository, text),
            Term::Field(TextField::Branch, text) => entry
                .label
                .as_ref()
                .is_some_and(|label| contains(&label.branch, text)),
            Term::Field(TextField::Eval, text) => contains(&entry.eval, text),
            Term::Field(TextField::Request, text) => contains(&entry.request, text),
            Term::Field(TextField::Reviewer, text) => entry
                .claim
                .reviewer()
                .is_some_and(|reviewer| contains(reviewer, text)),
            Term::Field(TextField::Tag, text) => entry.tags.iter().any(|tag| contains(tag, text)),
            Term::Kind(kind) => entry.kind == *kind,
            Term::Status(RequestStatus::WaitingHuman) => entry.status.is_human_wait(),
            Term::Status(status) => entry.status == *status,
            Term::Is(Is::Mine) => matches!(entry.claim, Claim::Mine(_)),
            Term::Is(Is::Claimed) => entry.claim != Claim::Unclaimed,
            Term::Is(Is::Unclaimed) => {
                entry.status.is_human_wait() && entry.claim == Claim::Unclaimed
            }
        })
    }

    /// What the state read shows besides the requests: their Runs' definitions only for a
    /// `tag:` term, the one term matched against them. They hold a Run's whole graph, which
    /// for a large Run costs seconds and gigabytes to decode for each batch.
    fn shown(&self) -> store::Shown {
        if self
            .terms
            .iter()
            .any(|term| matches!(term, Term::Field(TextField::Tag, _)))
        {
            store::Shown::Definitions
        } else {
            store::Shown::Requests
        }
    }

    fn wants_labels(&self) -> bool {
        self.places
            || self
                .terms
                .iter()
                .any(|term| matches!(term, Term::Field(TextField::Branch, _)))
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
                .chain(std::iter::once(WAITING.to_owned()))
                .filter(|key| key.starts_with(&lower))
                .collect()
        }
        Some((name, partial)) => match Key::parse(name) {
            None => Vec::new(),
            Some(key) => {
                let partial = partial.trim_matches('"').to_lowercase();
                let values: BTreeSet<String> = match key {
                    Key::Text(TextField::Repo) => entries
                        .iter()
                        .map(|entry| {
                            if let Some(label) = &entry.label {
                                return label.project.clone();
                            }
                            entry.repository.file_name().map_or_else(
                                || crate::platform::path_text(&entry.repository),
                                crate::platform::display_name,
                            )
                        })
                        .collect(),
                    Key::Text(TextField::Branch) => entries
                        .iter()
                        .filter_map(|entry| entry.label.as_ref())
                        .filter(|label| !label.branch.is_empty())
                        .map(|label| label.branch.clone())
                        .collect(),
                    Key::Text(TextField::Eval) => {
                        entries.iter().map(|entry| entry.eval.clone()).collect()
                    }
                    Key::Text(TextField::Request) => {
                        entries.iter().map(|entry| entry.request.clone()).collect()
                    }
                    Key::Text(TextField::Reviewer) => entries
                        .iter()
                        .filter_map(|entry| entry.claim.reviewer().map(str::to_owned))
                        .collect(),
                    Key::Text(TextField::Tag) => entries
                        .iter()
                        .flat_map(|entry| entry.tags.iter().cloned())
                        .collect(),
                    Key::Kind | Key::Type => kind_values().collect(),
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
    suggest_in(state, text, reviewer, false).await
}

pub async fn suggest_places(
    state: &Path,
    text: &str,
    reviewer: &ReviewerId,
) -> Result<Completion, String> {
    suggest_in(state, text, reviewer, true).await
}

async fn suggest_in(
    state: &Path,
    text: &str,
    reviewer: &ReviewerId,
    places: bool,
) -> Result<Completion, String> {
    let (keep, term) = last_term(text);
    let key = term
        .split_once(':')
        .filter(|_| !term.starts_with('"'))
        .and_then(|(name, _)| Key::parse(name));
    let from_requests = matches!(key, Some(Key::Text(_)));
    let shown = match key {
        Some(Key::Text(TextField::Tag)) => store::Shown::Definitions,
        _ => keep
            .parse::<Query>()
            .map_or(store::Shown::Requests, |query| query.shown()),
    };
    let entries = match keep.parse::<Query>() {
        // Tags are completed from the definitions the matches' Runs saved.
        Ok(mut query) if from_requests => {
            query.places = places || matches!(key, Some(Key::Text(TextField::Branch)));
            query.live = places;
            page(state, &query, reviewer, 0, shown)
                .await?
                .requests
                .into_iter()
                .map(|(_, entry)| entry)
                .collect()
        }
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

/// The exact saved request and whether it appears in the current search, regardless of page.
#[derive(Debug, Clone)]
pub struct Opened {
    pub view: RequestView,
    pub outside_reason: Option<String>,
}

impl Opened {
    pub fn outside_filter(&self) -> bool {
        self.outside_reason.is_some()
    }
}

/// Resolve a direct reference without changing its query or redirecting to an original request.
pub async fn open(
    state: &Path,
    id: &RequestId,
    query: &Query,
    reviewer: &ReviewerId,
) -> Result<Opened, String> {
    let view = store::read_request_by_id(state, id, query.shown()).await?;
    let repo = store::read_request_repo_by_id(state, id.clone()).await?;
    let mut entry = Entry::new(&view, &repo, reviewer);
    if query.wants_labels() {
        entry.label = labels(state, vec![view.request.run_id.clone()])
            .await?
            .remove(&view.request.run_id);
    }
    let outside_reason = if query.live && !repo.is_dir() {
        Some("This review's repository folder no longer exists.".into())
    } else if Box::pin(follows_waiting(state, &view, query.live)).await {
        Some("This review shares the original request's sign-off.".into())
    } else if query.matches(&entry) {
        None
    } else {
        Some(outside_reason(&entry))
    };
    Ok(Opened {
        view,
        outside_reason,
    })
}

fn outside_reason(entry: &Entry) -> String {
    if entry.kind != ProfileKind::Human {
        return "This is not a Human review.".into();
    }
    match (&entry.claim, entry.status) {
        (Claim::Mine(_), _) => "Claimed by you.".into(),
        (Claim::Other(reviewer), _) => format!("Claimed by {reviewer}."),
        (_, RequestStatus::Green | RequestStatus::Red) => "This review was submitted.".into(),
        _ => "This review does not match the current filter.".into(),
    }
}

/// Up to [`PAGE`] requests matching `query` from position `start` on, read in bounded batches.
///
/// A waiting Human request that joined another waiting request's execution (a follower) is
/// listed once, as the original that records its definition and receives every action; a
/// follower whose original no longer waits is listed as itself. The first page first ends the
/// sign-offs whose repository folder is gone ([`store::retire_removed_signoffs`]): nothing can
/// be reviewed in them any more.
pub async fn search(
    state: &Path,
    query: &Query,
    reviewer: &ReviewerId,
    start: u64,
) -> Result<Found, String> {
    if start == 0 {
        store::retire_removed_signoffs(state).await?;
    }
    page(state, query, reviewer, start, query.shown()).await
}

/// One [`search`] page, its views showing what `shown` names besides their requests.
async fn page(
    state: &Path,
    query: &Query,
    reviewer: &ReviewerId,
    start: u64,
    shown: store::Shown,
) -> Result<Found, String> {
    let mut found = Found::default();
    read_matching(
        state,
        query,
        reviewer,
        start,
        shown,
        |view, entry, position| {
            found.requests.push((view, entry));
            if found.requests.len() < PAGE {
                return ControlFlow::Continue(());
            }
            found.next = Some(position + 1);
            ControlFlow::Break(())
        },
    )
    .await?;
    Ok(found)
}

/// How many requests `query` finds on all its pages together, each counted as [`search`]
/// lists it.
pub async fn count(state: &Path, query: &Query, reviewer: &ReviewerId) -> Result<u64, String> {
    let mut count = 0;
    read_matching(state, query, reviewer, 0, query.shown(), |_, _, _| {
        count += 1;
        ControlFlow::Continue(())
    })
    .await?;
    Ok(count)
}

/// Pass each request matching `query` from position `start` on to `found` with its position,
/// in the state's request order, until `found` breaks or the requests end. Each view shows what
/// `shown` names besides its request.
async fn read_matching(
    state: &Path,
    query: &Query,
    reviewer: &ReviewerId,
    start: u64,
    shown: store::Shown,
    mut found: impl FnMut(RequestView, Entry, u64) -> ControlFlow<()>,
) -> Result<(), String> {
    let statuses = query.statuses();
    let mut position = start;
    let mut exists = std::collections::BTreeMap::new();
    loop {
        let batch = if query.live {
            store::read_review_batch(state, &statuses, position, BATCH, shown).await?
        } else {
            store::read_request_batch(state, &statuses, position, BATCH, shown).await?
        };
        let read = batch.len() as u64;
        let labels = if query.wants_labels() {
            let ids: BTreeSet<_> = batch
                .iter()
                .map(|(view, _)| view.request.run_id.clone())
                .collect();
            labels(state, ids.into_iter().collect()).await?
        } else {
            Labels::new()
        };
        for (offset, (view, repo)) in batch.into_iter().enumerate() {
            if (query.live && !*exists.entry(repo.clone()).or_insert_with(|| repo.is_dir()))
                || Box::pin(follows_waiting(state, &view, query.live)).await
            {
                continue;
            }
            let mut entry = Entry::new(&view, &repo, reviewer);
            entry.label = labels.get(&view.request.run_id).cloned();
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

async fn labels(state: &Path, ids: Vec<crate::types::RunId>) -> Result<Labels, String> {
    let catalog = store::read_run_places(state, ids).await?;
    crate::task::joined(
        tokio::task::spawn_blocking(move || {
            let mut legacy = std::collections::BTreeMap::new();
            catalog
                .into_iter()
                .map(|(id, run)| {
                    let label = if run.repository.common_dir.is_some() {
                        Label::for_identity(&run.repo_path, &run.repository)
                    } else {
                        legacy
                            .entry(run.repo_path.clone())
                            .or_insert_with(|| Label::at(&run.repo_path))
                            .clone()
                    };
                    (id, label)
                })
                .collect()
        })
        .await,
    )
    .map_err(|error| error.to_string())
}

/// A waiting follower whose original still waits: the original's own row lists it.
async fn follows_waiting(state: &Path, view: &RequestView, live: bool) -> bool {
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
    // Only the original's status matters, never what its Run's definitions hold.
    if !store::read_request_by_id(state, source, store::Shown::Requests)
        .await
        .is_ok_and(|original| original.request.status().is_human_wait())
    {
        return false;
    }
    !live
        || store::read_request_repo_by_id(state, source.clone())
            .await
            .is_ok_and(|repo| repo.is_dir())
}
