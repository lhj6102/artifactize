//! The pane's search: free text and `key:value` terms that narrow the branches and the waiting
//! reviews alike, and the order of the branches.
//!
//! Terms are ANDed. A plain word matches, case-insensitively, part of a project or a branch,
//! or of a waiting review's eval. A `key:value` term restricts the match to one field:
//!
//! | Key | Matches |
//! |---|---|
//! | `repo:` | part of the project's name |
//! | `branch:` | part of the branch's name |
//! | `sort:` | not a filter: `recent` (the most recent Run first, branches without Runs last) or `name` (by project, then branch) |
//!
//! Values with spaces are quoted, and a quoted word is always free text, as in the review
//! search ([`crate::review::search`]), whose words this one reads.
use super::{Branch, Place, Waiting};
use crate::review::search::{self, Completion};
use std::collections::BTreeSet;

/// The search the pane starts with: every branch, the most recent Run first.
pub const DEFAULT: &str = "sort:recent";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Repo,
    Branch,
    Sort,
}

impl Key {
    const ALL: [Self; 3] = [Self::Repo, Self::Branch, Self::Sort];

    fn name(self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Branch => "branch",
            Self::Sort => "sort",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|key| key.name().eq_ignore_ascii_case(name))
    }
}

/// How the branches are ordered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sort {
    /// The branch with the most recent Run first; branches without Runs last.
    #[default]
    Recent,
    /// By project, then branch.
    Name,
}

impl Sort {
    const ALL: [Self; 2] = [Self::Recent, Self::Name];

    fn name(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Name => "name",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Term {
    /// Lowercased text found in any field.
    Text(String),
    /// Lowercased text found in the project's name.
    Repo(String),
    /// Lowercased text found in the branch's name.
    Branch(String),
}

/// A parsed search; every term must match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    terms: Vec<Term>,
    sort: Sort,
}

impl std::str::FromStr for Query {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let mut query = Self::default();
        let mut sorted = false;
        for (word, quoted) in search::words(text) {
            let Some((name, value)) = word.split_once(':').filter(|_| !quoted) else {
                query.terms.push(Term::Text(word.to_lowercase()));
                continue;
            };
            let key = Key::parse(name).ok_or_else(|| {
                format!(
                    "Unknown key `{name}:`; use repo:, branch: or sort:, or quote the word to search for it."
                )
            })?;
            if value.is_empty() {
                return Err(format!("`{}:` needs a value.", key.name()));
            }
            let lower = value.to_lowercase();
            match key {
                Key::Repo => query.terms.push(Term::Repo(lower)),
                Key::Branch => query.terms.push(Term::Branch(lower)),
                Key::Sort if sorted => return Err("Use one sort: term.".into()),
                Key::Sort => {
                    query.sort = Sort::ALL
                        .into_iter()
                        .find(|sort| sort.name() == lower)
                        .ok_or_else(|| {
                            format!("Unknown order `sort:{value}`; use sort:recent or sort:name.")
                        })?;
                    sorted = true;
                }
            }
        }
        Ok(query)
    }
}

fn contains(field: &str, text: &str) -> bool {
    field.to_lowercase().contains(text)
}

impl Query {
    pub fn sort(&self) -> Sort {
        self.sort
    }

    fn matches(&self, project: &str, branch: &str, eval: Option<&str>) -> bool {
        self.terms.iter().all(|term| match term {
            Term::Text(text) => [Some(project), Some(branch), eval]
                .into_iter()
                .flatten()
                .any(|field| contains(field, text)),
            Term::Repo(text) => contains(project, text),
            Term::Branch(text) => contains(branch, text),
        })
    }

    pub fn branch(&self, branch: &Branch) -> bool {
        self.matches(&branch.project, &branch.name, None)
    }

    pub fn waiting(&self, waiting: &Waiting) -> bool {
        self.matches(
            &waiting.label.project,
            &waiting.label.branch,
            Some(&waiting.entry.eval),
        )
    }
}

/// How `sort` orders two branches; ties go by project, then branch.
fn compare(sort: Sort, a: &Branch, b: &Branch) -> std::cmp::Ordering {
    let names = (&a.project, &a.name, &a.place).cmp(&(&b.project, &b.name, &b.place));
    match sort {
        // Newest first: a later Run is the greater time; without Runs is the least.
        Sort::Recent => b.latest().cmp(&a.latest()).then(names),
        Sort::Name => names,
    }
}

/// Order `branches` by `sort`.
pub(super) fn order(branches: &mut [Branch], sort: Sort) {
    branches.sort_by(|a, b| compare(sort, a, b));
}

/// The branches and waiting reviews `query` keeps: the branches `starred` first, each group in
/// the query's order, and the waiting reviews as they came (newest first).
pub fn show<'a>(
    query: &Query,
    branches: &'a [Branch],
    waiting: &'a [Waiting],
    starred: &BTreeSet<Place>,
) -> (Vec<&'a Branch>, Vec<&'a Waiting>) {
    let mut kept: Vec<&Branch> = branches
        .iter()
        .filter(|branch| query.branch(branch))
        .collect();
    let first = |branch: &Branch| !starred.contains(&branch.place);
    kept.sort_by(|a, b| {
        first(a)
            .cmp(&first(b))
            .then_with(|| compare(query.sort, a, b))
    });
    let waiting = waiting
        .iter()
        .filter(|waiting| query.waiting(waiting))
        .collect();
    (kept, waiting)
}

/// The candidates for the search's last term: keys whose names start with it, or after a key
/// the projects or branches listed, or the orders.
pub fn complete(text: &str, branches: &[Branch], waiting: &[Waiting]) -> Completion {
    let (keep, term) = search::last_term(text);
    let candidates = match term.split_once(':').filter(|_| !term.starts_with('"')) {
        // A quoted word is searched for literally: nothing to complete.
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
                    Key::Repo => branches
                        .iter()
                        .map(|branch| branch.project.clone())
                        .chain(waiting.iter().map(|waiting| waiting.label.project.clone()))
                        .collect(),
                    Key::Branch => branches
                        .iter()
                        .map(|branch| branch.name.clone())
                        .chain(waiting.iter().map(|waiting| waiting.label.branch.clone()))
                        .collect(),
                    Key::Sort => Sort::ALL
                        .iter()
                        .map(|sort| sort.name().to_owned())
                        .collect(),
                };
                values
                    .into_iter()
                    .filter(|value| !value.is_empty() && value.to_lowercase().contains(&partial))
                    .map(|value| search::quote(key.name(), &value))
                    .collect()
            }
        },
    };
    Completion {
        keep: keep.to_owned(),
        candidates,
    }
}
