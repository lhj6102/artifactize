use std::{path::Path, sync::Arc};

use serde::Serialize;

use tokio_rusqlite::Connection;

use super::{
    DATABASE, Execution, HumanClaim, Request,
    receipts::{Error, check_files, schema_initialized},
};
use crate::workspace::{canonical_target, outside_workspace};

#[derive(Debug, Clone, Serialize)]
pub struct RequestView {
    #[serde(flatten)]
    pub request: Request,
    pub claim: Option<HumanClaim>,
    pub execution: Option<Execution>,
    pub definition: Option<RequestDefinition>,
}

/// The saved definitions of a request's eval and Artifact, shared by every view of the Run.
#[derive(Debug, Clone, Serialize)]
pub struct RequestDefinition {
    pub eval: Arc<super::definitions::Eval>,
    pub artifact: Arc<super::definitions::Artifact>,
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
    read(
        state,
        Filter {
            id: Some(id),
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
        Some(id) => read_request(state, id).await,
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
/// `offset` of every Run's requests, newest Run first, each with its Run's repository.
pub async fn read_request_batch(
    state: &Path,
    statuses: &[crate::types::RequestStatus],
    offset: u64,
    limit: u32,
) -> Result<Vec<(RequestView, std::path::PathBuf)>, String> {
    read_with_repo(
        state,
        Filter {
            statuses,
            slice: Some((offset, limit)),
            ..Filter::default()
        },
    )
    .await
}

/// WAITING_HUMAN requests, newest Run first, from the canonical `repo`'s Runs or every Run.
pub async fn read_waiting(state: &Path, repo: Option<&Path>) -> Result<Vec<RequestView>, String> {
    read(
        state,
        Filter {
            repo,
            waiting: true,
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

/// Human action setup needs only the owning repository, not another full Run/request audit.
pub async fn read_request_repo(
    state: &Path,
    id: &str,
) -> Result<(RequestView, std::path::PathBuf), String> {
    read_with_repo(
        state,
        Filter {
            id: Some(id),
            ..Filter::default()
        },
    )
    .await?
    .pop()
    .ok_or_else(|| "Review request not found.".into())
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
    check_files(&state)?;
    if !state
        .join(DATABASE)
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Ok(Vec::new());
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
    let connection = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map_err(|e| e.to_string())?;
    let result = connection
        .call(move |db| -> Result<_, Error> {
            db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
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
            };
            let rows = rows(&transaction, &filter)?;
            let ids: std::collections::BTreeSet<_> = rows.iter().map(|row| &row.run).collect();
            let mut statement = transaction.prepare(
                "SELECT id,repo,data FROM runs WHERE id IN (SELECT value FROM json_each(?))",
            )?;
            let runs = statement
                .query_map([serde_json::to_string(&ids)?], |row| {
                    Ok((
                        row.get::<_, crate::types::RunId>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut indexes = std::collections::BTreeMap::new();
            let mut repos = Vec::new();
            for (id, repo, data) in runs {
                let Some(run) = super::unreadable::evidence::<super::Run>("run", &data) else {
                    continue;
                };
                repos.push(std::path::PathBuf::from(repo));
                indexes.insert(id, (DefinitionIndex::new(&run), run.repo_path));
            }
            crate::workspace::outside_workspaces(repos.iter().map(|repo| repo.as_path()), &state)
                .map_err(|error| Error::Invalid(error.to_string()))?;
            let views = rows
                .into_iter()
                .filter_map(|row| {
                    indexes
                        .get(&row.run)
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    pub id: crate::types::RequestId,
    pub run: crate::types::RunId,
    pub eval: crate::types::EvalId,
    pub ordinal: i64,
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
        "SELECT q.id,q.run_id,q.data,e.data,h.id,h.claimed_by,h.claimed_at,q.eval_id,q.ordinal,e.id
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
                ordinal: row.get(8)?,
                data: row.get(2)?,
                execution: row.get(3)?,
                execution_id: row.get(9)?,
                claim: id
                    .zip(reviewer)
                    .zip(at)
                    .map(|((id, reviewer), at)| (id, reviewer, at)),
            })
        })?
        .collect::<Result<_, _>>()?)
}

/// Saved definitions are decoded/indexed once per Run, never once per request. First duplicate
/// Eval wins, matching SQLite's former json_each lookup; missing/null definitions remain absent.
pub(super) struct DefinitionIndex {
    artifacts:
        std::collections::BTreeMap<crate::types::ArtifactName, Arc<super::definitions::Artifact>>,
    evals: std::collections::BTreeMap<crate::types::EvalId, Arc<super::definitions::Eval>>,
}
impl DefinitionIndex {
    pub fn new(run: &super::Run) -> Self {
        let mut evals = std::collections::BTreeMap::new();
        if let Some(graph) = run.definitions.graph() {
            for eval in graph.evals() {
                evals
                    .entry(eval.id.clone())
                    .or_insert_with(|| Arc::new(eval.clone()));
            }
        }
        Self {
            artifacts: run
                .definitions
                .graph()
                .map(|graph| {
                    graph
                        .artifacts()
                        .map(|(id, artifact)| (id.clone(), Arc::new(artifact.clone())))
                        .collect()
                })
                .unwrap_or_default(),
            evals,
        }
    }
}
impl Row {
    pub fn view(&self, index: &DefinitionIndex) -> Option<RequestView> {
        let request = super::unreadable::evidence::<Request>("request", &self.data)?;
        let execution = self
            .execution
            .as_deref()
            .and_then(|data| super::unreadable::evidence("execution", data));
        Some(self.with_evidence(index, request, execution))
    }
    pub fn view_cached(
        &self,
        index: &DefinitionIndex,
        audit: &mut super::unreadable::Audit,
    ) -> Option<RequestView> {
        let request = audit
            .take_request(&self.id, &self.data)
            .or_else(|| super::unreadable::evidence("request", &self.data))?;
        let execution = self.execution.as_deref().and_then(|data| {
            self.execution_id
                .as_ref()
                .and_then(|id| audit.take_execution(id, data))
                .or_else(|| super::unreadable::evidence("execution", data))
        });
        Some(self.with_evidence(index, request, execution))
    }
    fn with_evidence(
        &self,
        index: &DefinitionIndex,
        request: Request,
        execution: Option<Execution>,
    ) -> RequestView {
        let definition = index
            .evals
            .get(&self.eval)
            .zip(index.artifacts.get(&request.target))
            .map(|(eval, artifact)| RequestDefinition {
                eval: eval.clone(),
                artifact: artifact.clone(),
            });
        RequestView {
            request,
            definition,
            execution,
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
                    cache.selected(db, state, &id, None, true)
                })?
                .ok_or_else(|| "Run not found.".into())
        })
        .await,
    )
    .map_err(|error| error.to_string())?
}
