use std::{collections::HashMap, path::Path, sync::Arc};

use serde::Serialize;

use rusqlite::OptionalExtension;
use tokio_rusqlite::Connection;

use super::{
    DATABASE, Execution, HumanClaim, Request,
    definitions::Graph,
    receipts::{Error, check_files, schema_initialized},
};
use crate::workspace::{canonical_target, outside_workspace};

/// A request with what it shows. Its decoded records are shared: a copy of the view, and the
/// Run's view of the same read, keep the one request and execution, so a snapshot that is
/// read again copies only the requests that changed.
#[derive(Debug, Clone, Serialize)]
pub struct RequestView {
    #[serde(flatten)]
    pub request: Arc<Request>,
    pub claim: Option<HumanClaim>,
    pub execution: Option<Arc<Execution>>,
    pub definition: Option<RequestDefinition>,
}

/// The saved definitions of a request's eval and Artifact.
#[derive(Clone)]
pub struct RequestDefinition(Place);
#[derive(Clone)]
enum Place {
    /// A place in the Run's one decoded graph, which every view of an opened Run shares.
    Graph {
        graph: Arc<Graph>,
        /// The eval's position in the graph's evals.
        eval: usize,
        artifact: crate::types::ArtifactName,
    },
    /// Copies of its own eval and Artifact, for a view read apart from its Run, such as a
    /// waiting review's, which may be kept long after the rest of the Run is gone.
    Own {
        eval: Arc<super::definitions::Eval>,
        artifact: Arc<super::definitions::Artifact>,
    },
}
impl RequestDefinition {
    pub fn eval(&self) -> &super::definitions::Eval {
        match &self.0 {
            Place::Graph { graph, eval, .. } => &graph.evals()[*eval],
            Place::Own { eval, .. } => eval,
        }
    }
    pub fn artifact(&self) -> &super::definitions::Artifact {
        match &self.0 {
            Place::Graph {
                graph, artifact, ..
            } => graph
                .artifact(artifact)
                .expect("an indexed request's Artifact is in its graph"),
            Place::Own { artifact, .. } => artifact,
        }
    }
}
impl Serialize for RequestDefinition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut definition = serializer.serialize_struct("RequestDefinition", 2)?;
        definition.serialize_field("eval", self.eval())?;
        definition.serialize_field("artifact", self.artifact())?;
        definition.end()
    }
}
impl std::fmt::Debug for RequestDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestDefinition")
            .field("eval", self.eval())
            .field("artifact", self.artifact())
            .finish()
    }
}

#[derive(Default)]
struct Filter<'a> {
    run: Option<&'a str>,
    id: Option<&'a str>,
    repo: Option<&'a Path>,
    waiting: bool,
    session: Option<&'a str>,
    statuses: &'a [crate::types::RequestStatus],
    /// A bounded slice of the ordered requests: offset and count.
    slice: Option<(u64, u32)>,
    shown: Shown,
}

/// What the views of a read show besides their requests.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// The definitions their Runs saved, which hold the Runs' whole graphs: each Run read is
    /// decoded whole.
    #[default]
    Definitions,
    /// No definitions: only the Runs' repositories are read, never their records.
    Requests,
}

pub async fn read_requests(state: &Path, run: Option<&str>) -> Result<Vec<RequestView>, String> {
    read(
        state,
        Filter {
            run,
            ..Filter::default()
        },
    )
    .await
}

pub async fn read_requests_in(
    state: &Path,
    run: Option<&str>,
    repo: Option<&Path>,
) -> Result<Vec<RequestView>, String> {
    read(
        state,
        Filter {
            run,
            repo,
            ..Filter::default()
        },
    )
    .await
}

pub async fn read_request(state: &Path, id: &str) -> Result<RequestView, String> {
    read_request_as(state, id, Shown::Definitions).await
}

/// Request `id` with what `shown` names besides it.
pub async fn read_request_as(state: &Path, id: &str, shown: Shown) -> Result<RequestView, String> {
    read(
        state,
        Filter {
            id: Some(id),
            shown,
            ..Filter::default()
        },
    )
    .await?
    .pop()
    .ok_or_else(|| "Review request not found.".into())
}

/// The request whose Agent review ran as session `id`, if this state has it.
pub async fn read_session_request(state: &Path, id: &str) -> Result<Option<RequestView>, String> {
    Ok(read(
        state,
        Filter {
            session: Some(id),
            ..Filter::default()
        },
    )
    .await?
    .pop())
}

/// The request that owns `view`'s shared execution: a follower of another Run joins the
/// original waiting request, which alone records the Human definition (tools and owner
/// schemas), the claim and the result. `view` itself when it is the original.
pub async fn read_original(state: &Path, view: &RequestView) -> Result<RequestView, String> {
    read_original_as(state, view, Shown::Definitions).await
}

/// The request that owns `view`'s shared execution, read with what `shown` names besides it.
pub async fn read_original_as(
    state: &Path,
    view: &RequestView,
    shown: Shown,
) -> Result<RequestView, String> {
    let original = view
        .execution
        .as_ref()
        .map(|execution| &execution.provenance.request_id)
        .or_else(|| {
            view.request
                .provenance
                .as_ref()
                .map(|source| &source.request_id)
        });
    match original.filter(|id| *id != &view.request.id) {
        Some(id) => read_request_as(state, id, shown).await,
        None => Ok(view.clone()),
    }
}

/// One entry per waiting Human sign-off: followers collapse onto the original request that
/// owns their shared execution, at the position of the first entry that names it.
pub async fn read_waiting_originals(
    state: &Path,
    repo: Option<&Path>,
) -> Result<Vec<RequestView>, String> {
    let waiting = read_waiting(state, repo).await?;
    let by_id: std::collections::BTreeMap<_, _> = waiting
        .iter()
        .map(|view| (&view.request.id, view))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut originals: Vec<RequestView> = Vec::with_capacity(waiting.len());
    for view in &waiting {
        let source = view
            .execution
            .as_ref()
            .map_or(&view.request.id, |execution| {
                &execution.provenance.request_id
            });
        if seen.contains(source) {
            continue;
        }
        let original = match by_id.get(source) {
            Some(original) => Some((*original).clone()),
            None => read_original(state, view).await.ok(),
        };
        // A follower whose original is gone or no longer waits stays listed as itself.
        let original = original
            .filter(|original| original.request.status().is_human_wait())
            .unwrap_or_else(|| view.clone());
        if seen.insert(original.request.id.clone()) {
            originals.push(original);
        }
    }
    Ok(originals)
}

/// Up to `limit` requests with one of `statuses` (any status when empty) from position
/// `offset` of every Run's requests, newest Run first, each with its Run's repository and what
/// `shown` names besides it.
pub async fn read_request_batch(
    state: &Path,
    statuses: &[crate::types::RequestStatus],
    offset: u64,
    limit: u32,
    shown: Shown,
) -> Result<Vec<(RequestView, std::path::PathBuf)>, String> {
    read_with_repo(
        state,
        Filter {
            statuses,
            slice: Some((offset, limit)),
            shown,
            ..Filter::default()
        },
    )
    .await
}

/// WAITING_HUMAN requests, newest Run first, from the canonical `repo`'s Runs or every Run.
/// Their views have no definitions: what waits, not what it reviews, so the read does not grow
/// with the waiting Runs' workspaces.
pub async fn read_waiting(state: &Path, repo: Option<&Path>) -> Result<Vec<RequestView>, String> {
    read(
        state,
        Filter {
            repo,
            waiting: true,
            shown: Shown::Requests,
            ..Filter::default()
        },
    )
    .await
}

async fn read(state: &Path, filter: Filter<'_>) -> Result<Vec<RequestView>, String> {
    Ok(read_with_repo(state, filter)
        .await?
        .into_iter()
        .map(|(view, _)| view)
        .collect())
}

/// The repository of the Run that request `id` belongs to, where its Human actions are
/// recorded. Only the request's Run row is read, never the Run's record or definitions, so a
/// Human action finds its receipts at once however large the Run's workspace is.
pub async fn read_request_repo(state: &Path, id: &str) -> Result<std::path::PathBuf, String> {
    let not_found = || "Review request not found.".to_owned();
    let id: crate::types::RequestId = id.parse()?;
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    let Some(connection) = open_read(&state).await? else {
        return Err(not_found());
    };
    let result = connection
        .call(move |db| -> Result<_, Error> {
            super::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(None);
            }
            let repo = transaction
                .query_row(
                    "SELECT r.repo FROM requests q JOIN runs r ON r.id=q.run_id WHERE q.id=?",
                    [&id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(std::path::PathBuf::from);
            transaction.commit()?;
            if let Some(repo) = &repo {
                crate::workspace::outside_workspaces(std::iter::once(repo.as_path()), &state)
                    .map_err(|error| Error::Invalid(error.to_string()))?;
            }
            Ok(repo)
        })
        .await
        .map_err(|e| e.to_string());
    super::connection::finish(connection, result)
        .await?
        .ok_or_else(not_found)
}

/// A read-only connection to the database of the canonical `state`, or none before it has one.
async fn open_read(state: &Path) -> Result<Option<Connection>, String> {
    check_files(state)?;
    if !state
        .join(DATABASE)
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Ok(None);
    }
    Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map(Some)
    .map_err(|e| e.to_string())
}

async fn read_with_repo(
    state: &Path,
    filter: Filter<'_>,
) -> Result<Vec<(RequestView, std::path::PathBuf)>, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    let repo = filter
        .repo
        .map(canonical_target)
        .transpose()
        .map_err(|e| e.to_string())?;
    if let Some(repo) = &repo {
        outside_workspace(repo, &state).map_err(|e| e.to_string())?;
    }
    let run = filter
        .run
        .map(str::parse::<crate::types::RunId>)
        .transpose()?;
    let id = filter
        .id
        .map(str::parse::<crate::types::RequestId>)
        .transpose()?;
    let repo = repo.map(|repo| crate::platform::path_text(&repo));
    let waiting = filter.waiting;
    let statuses = filter.statuses.to_vec();
    let slice = filter.slice;
    let session = filter
        .session
        .map(str::parse::<crate::types::SessionId>)
        .transpose()?;
    let shown = filter.shown;
    let Some(connection) = open_read(&state).await? else {
        return Ok(Vec::new());
    };
    let result = connection
        .call(move |db| -> Result<_, Error> {
            super::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Vec::new());
            }
            let filter = Query {
                run,
                id,
                repo,
                waiting,
                session,
                statuses,
                slice,
                ids: None,
            };
            let rows = rows(&transaction, &filter)?;
            let ids: std::collections::BTreeSet<_> = rows.iter().map(|row| &row.run).collect();
            let mut statement = transaction.prepare(match shown {
                Shown::Definitions => "SELECT r.id,r.repo,r.data,d.data FROM runs r LEFT JOIN run_definitions d ON d.run_id=r.id WHERE r.id IN (SELECT value FROM json_each(?))",
                Shown::Requests => "SELECT r.id,r.repo,NULL,NULL FROM runs r WHERE r.id IN (SELECT value FROM json_each(?))",
            })?;
            let runs = statement
                .query_map([serde_json::to_string(&ids)?], |row| {
                    Ok((
                        row.get::<_, crate::types::RunId>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut indexes = std::collections::BTreeMap::new();
            let mut repos = Vec::new();
            for (id, repo, state, definitions) in runs {
                let index = match state {
                    None => DefinitionIndex::none(),
                    Some(state) => {
                        match super::run_record::whole(&id, &state, definitions.as_deref()) {
                            Ok(run) => DefinitionIndex::apart(&run),
                            Err(unreadable) => {
                                super::unreadable::skipped(&unreadable);
                                continue;
                            }
                        }
                    }
                };
                let repo = std::path::PathBuf::from(repo);
                repos.push(repo.clone());
                indexes.insert(id, (index, repo));
            }
            crate::workspace::outside_workspaces(repos.iter().map(|repo| repo.as_path()), &state)
                .map_err(|error| Error::Invalid(error.to_string()))?;
            let views = rows
                .into_iter()
                .filter_map(|row| {
                    indexes
                        .get_mut(&row.run)
                        .and_then(|(index, repo)| row.view(index).map(|view| (view, repo.clone())))
                })
                .collect();
            drop(statement);
            transaction.commit()?;
            Ok(views)
        })
        .await
        .map_err(|e| e.to_string());
    super::connection::finish(connection, result).await
}

/// Only present predicates are emitted, so Run and request lookups use their existing indexes.
#[derive(Default)]
pub(super) struct Query {
    pub run: Option<crate::types::RunId>,
    pub id: Option<crate::types::RequestId>,
    pub repo: Option<String>,
    pub waiting: bool,
    pub session: Option<crate::types::SessionId>,
    pub statuses: Vec<crate::types::RequestStatus>,
    pub slice: Option<(u64, u32)>,
    /// Only these requests.
    pub ids: Option<Vec<crate::types::RequestId>>,
}

#[derive(Debug, Clone)]
pub(super) struct Row {
    pub id: crate::types::RequestId,
    pub run: crate::types::RunId,
    pub eval: crate::types::EvalId,
    pub data: String,
    pub execution: Option<String>,
    pub execution_id: Option<crate::types::ExecutionId>,
    pub claim: Option<(
        crate::types::RequestId,
        crate::types::ReviewerId,
        crate::types::Timestamp,
    )>,
}

pub(super) fn rows(db: &rusqlite::Connection, filter: &Query) -> Result<Vec<Row>, Error> {
    let mut predicates = Vec::new();
    let mut values: Vec<rusqlite::types::Value> = Vec::new();
    for (column, value) in [
        ("q.run_id", filter.run.as_ref().map(ToString::to_string)),
        ("q.id", filter.id.as_ref().map(ToString::to_string)),
        ("r.repo", filter.repo.clone()),
        (
            "json_extract(q.data,'$.sessionId')",
            filter.session.as_ref().map(ToString::to_string),
        ),
    ] {
        if let Some(value) = value {
            predicates.push(format!("{column}=?"));
            values.push(value.into());
        }
    }
    if filter.waiting {
        predicates.push("q.status IN ('WAITING_HUMAN','DEFERRED')".into());
    }
    if let Some(ids) = &filter.ids {
        predicates.push("q.id IN (SELECT value FROM json_each(?))".into());
        values.push(serde_json::to_string(ids)?.into());
    }
    if !filter.statuses.is_empty() {
        let marks = vec!["?"; filter.statuses.len()].join(",");
        predicates.push(format!("q.status IN ({marks})"));
        values.extend(
            filter
                .statuses
                .iter()
                .map(|status| rusqlite::types::Value::from(status.as_str().to_owned())),
        );
    }
    let predicate = if predicates.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", predicates.join(" AND "))
    };
    let limit = match filter.slice {
        Some((offset, count)) => {
            values.push(i64::from(count).into());
            values.push(i64::try_from(offset).unwrap_or(i64::MAX).into());
            " LIMIT ? OFFSET ?"
        }
        None => "",
    };
    let order = if filter.run.is_some() {
        "q.ordinal"
    } else {
        "r.rowid DESC,q.ordinal"
    };
    let mut statement = db.prepare_cached(&format!(
        "SELECT q.id,q.run_id,q.data,e.data,h.id,h.claimed_by,h.claimed_at,q.eval_id,e.id
        FROM requests q JOIN runs r ON r.id=q.run_id
        LEFT JOIN executions e ON e.id=q.execution_id
        LEFT JOIN requests h ON h.id=json_extract(e.data,'$.provenance.requestId') AND h.claimed_by IS NOT NULL
        {predicate} ORDER BY {order}{limit}"
    ))?;
    Ok(statement
        .query_map(rusqlite::params_from_iter(values), |row| {
            let id = row.get::<_, Option<crate::types::RequestId>>(4)?;
            let reviewer = row.get::<_, Option<crate::types::ReviewerId>>(5)?;
            let at = row.get::<_, Option<crate::types::Timestamp>>(6)?;
            Ok(Row {
                id: row.get(0)?,
                run: row.get(1)?,
                eval: row.get(7)?,
                data: row.get(2)?,
                execution: row.get(3)?,
                execution_id: row.get(8)?,
                claim: id
                    .zip(reviewer)
                    .zip(at)
                    .map(|((id, reviewer), at)| (id, reviewer, at)),
            })
        })?
        .collect::<Result<_, _>>()?)
}

/// Saved definitions are decoded once per Run and indexed by position, never copied whole.
/// First duplicate Eval wins, matching SQLite's former json_each lookup; missing/null
/// definitions remain absent.
pub(super) struct DefinitionIndex {
    graph: Option<Arc<Graph>>,
    evals: HashMap<crate::types::EvalId, usize>,
    /// The copies handed out so far, when each view keeps its own ([`Place::Own`]).
    copies: Option<Copies>,
}
#[derive(Default)]
struct Copies {
    evals: HashMap<usize, Arc<super::definitions::Eval>>,
    artifacts: HashMap<crate::types::ArtifactName, Arc<super::definitions::Artifact>>,
}
impl DefinitionIndex {
    /// The views share the Run's graph: an opened Run's, read as one.
    pub fn new(run: &super::Run) -> Self {
        let graph = run.definitions.share();
        let mut evals = HashMap::new();
        for (position, eval) in graph.iter().flat_map(|graph| graph.evals()).enumerate() {
            evals.entry(eval.id.clone()).or_insert(position);
        }
        Self {
            graph,
            evals,
            copies: None,
        }
    }
    /// Each view keeps copies of its own eval and Artifact, shared only by the views of this
    /// read: requests read apart from their Runs.
    fn apart(run: &super::Run) -> Self {
        Self {
            copies: Some(Copies::default()),
            ..Self::new(run)
        }
    }
    /// No definitions: the views show their requests alone.
    fn none() -> Self {
        Self {
            graph: None,
            evals: HashMap::new(),
            copies: None,
        }
    }
    fn definition(
        &mut self,
        eval: &crate::types::EvalId,
        artifact: &crate::types::ArtifactName,
    ) -> Option<RequestDefinition> {
        let graph = self.graph.as_ref()?;
        let saved = graph.artifact(artifact)?;
        let position = *self.evals.get(eval)?;
        Some(RequestDefinition(match &mut self.copies {
            None => Place::Graph {
                graph: graph.clone(),
                eval: position,
                artifact: artifact.clone(),
            },
            Some(copies) => Place::Own {
                eval: copies
                    .evals
                    .entry(position)
                    .or_insert_with(|| Arc::new(graph.evals()[position].clone()))
                    .clone(),
                artifact: copies
                    .artifacts
                    .entry(artifact.clone())
                    .or_insert_with(|| Arc::new(saved.clone()))
                    .clone(),
            },
        }))
    }
}
impl Row {
    pub fn view(&self, index: &mut DefinitionIndex) -> Option<RequestView> {
        let request = super::unreadable::evidence::<Request>("request", &self.data)?;
        let execution = self
            .execution
            .as_deref()
            .and_then(|data| super::unreadable::evidence("execution", data));
        Some(self.with_evidence(index, request, execution))
    }
    /// The request's view and why any of its records do not read. A request that does not read
    /// has no view; an execution that does not read leaves the view without it.
    pub fn read(
        &self,
        index: &mut DefinitionIndex,
    ) -> (Option<RequestView>, Vec<super::Unreadable>) {
        let mut unreadable = Vec::new();
        let execution = self
            .execution
            .as_deref()
            .zip(self.execution_id.as_ref())
            .and_then(|(data, id)| {
                super::unreadable::decode("execution", id, data)
                    .map_err(|record| unreadable.push(record))
                    .ok()
            });
        let view = super::unreadable::decode::<Request>("request", &self.id, &self.data)
            .map_err(|record| unreadable.push(record))
            .ok()
            .map(|request| self.with_evidence(index, request, execution));
        (view, unreadable)
    }
    fn with_evidence(
        &self,
        index: &mut DefinitionIndex,
        request: Request,
        execution: Option<Execution>,
    ) -> RequestView {
        let definition = index.definition(&self.eval, &request.target);
        RequestView {
            request: Arc::new(request),
            definition,
            execution: execution.map(Arc::new),
            claim: self.claim.as_ref().map(|(id, reviewer, at)| HumanClaim {
                request_id: id.clone(),
                reviewer: reviewer.clone(),
                claimed_at: *at,
            }),
        }
    }
}

/// Both selected-Run view shapes from the same short read transaction and decoded rows.
pub async fn read_run_requests(
    state: &Path,
    id: &str,
) -> Result<(super::RunView, Vec<RequestView>), String> {
    let state = state.to_path_buf();
    let id: crate::types::RunId = id.parse()?;
    crate::task::joined(
        tokio::task::spawn_blocking(move || {
            let mut reader = super::reader::Reader::default();
            reader
                .snapshot(&state, |db, cache, state| {
                    cache.selected(db, state, &id, true)
                })?
                .ok_or_else(|| "Run not found.".into())
        })
        .await,
    )
    .map_err(|error| error.to_string())?
}
