//! Run admission and cancellation share the Run row's write lock with scheduler saves.
use super::{Receipts, Request, Run, receipts::Error};
use crate::{
    runs::{Activity, Control, Worker},
    types::{RequestKey, RunId, RunStatus},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use super::run_record::load;

/// The saved Run's status and control, which a save merges with.
fn saved_control(
    db: &rusqlite::Connection,
    id: &RunId,
) -> Result<(RunStatus, Option<Control>), Error> {
    let (status, control): (RunStatus, Option<String>) = db.query_row(
        "SELECT status,json_extract(data,'$.control') FROM runs WHERE id=?",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((
        status,
        control
            .map(|text| serde_json::from_str(&text))
            .transpose()?,
    ))
}

/// Never replace concurrent admission/cancellation with an older scheduler clone.
pub(super) fn save(db: &rusqlite::Connection, mut run: Run) -> Result<(), Error> {
    let (status, saved) = saved_control(db, &run.id)?;
    if !status.is_open() && saved.is_some() {
        return Ok(());
    }
    let running = run.status().is_open();
    if let (Some(control), Some(saved)) = (&mut run.control, saved) {
        control.worker = saved.worker;
        control.cancellation_requested |= saved.cancellation_requested;
        if control.cancellation_requested
            && !running
            && control.failure_code != Some(crate::types::FailureCode::OwnerDied)
        {
            control.failure_code = Some(crate::types::FailureCode::Cancelled);
            run.state = super::RunState::Error {
                at: crate::broker::now(),
                error: Some("Run was cancelled.".into()),
            };
            if let Some(validation) = &mut run.validation.0 {
                validation.satisfied = crate::config::Field::Value(false);
            }
        }
        if running && control.cancellation_requested {
            control.activity = Activity::Cancelling;
        }
    }
    super::run_record::update(db, &run)
}

impl Receipts {
    pub(crate) async fn keyed_run(
        &self,
        repo: &std::path::Path,
        key: &RequestKey,
    ) -> Result<Option<RunId>, String> {
        let repo = crate::platform::path_text(repo);
        let key = key.clone();
        self.connection
            .call(move |db| -> Result<Option<RunId>, Error> {
                Ok(db
                    .query_row(
                        "SELECT id FROM runs WHERE repo=? AND request_key=?",
                        params![repo, key],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn reserve(&self, run: &Run, requests: &[Request]) -> Result<RunId, String> {
        let run = run.clone();
        let requests = requests.to_vec();
        self.connection.call(move |db| -> Result<RunId, Error> {
            let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let repo = crate::platform::path_text(&run.repo_path);
            if let Some(key) = &run.request_key
                && let Some(id) = transaction.query_row("SELECT id FROM runs WHERE repo=? AND request_key=?", params![repo,key], |row| row.get(0)).optional()? {
                return Ok(id);
            }
            super::run_record::insert(&transaction, &run)?;
            for (ordinal, request) in requests.iter().enumerate() {
                transaction.execute("INSERT INTO requests(id,run_id,eval_id,ordinal,status,data) VALUES (?,?,?,?,?,?)", params![request.id,run.id,request.eval_id,ordinal as i64,request.status(),serde_json::to_string(request)?])?;
            }
            transaction.commit()?;
            Ok(run.id)
        }).await.map_err(|error| error.to_string())
    }

    /// Change the Run's control under the Run row's write lock. `change` sees the saved status
    /// and control, and only the control is written back: neither the Run's state nor its
    /// definitions are decoded, however large its catalog is.
    pub(crate) async fn control(
        &self,
        id: &RunId,
        change: impl FnOnce(RunStatus, &mut Option<Control>) -> Result<(), String> + Send + 'static,
    ) -> Result<(RunStatus, Option<Control>), String> {
        let id = id.clone();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let (status, mut control) = saved_control(&transaction, &id)?;
                change(status, &mut control).map_err(Error::Invalid)?;
                let text = control.as_ref().map(serde_json::to_string).transpose()?;
                transaction.execute(
                    "UPDATE runs SET data=CASE WHEN ?1 IS NULL THEN json_remove(data,'$.control') ELSE json_set(data,'$.control',json(?1)) END WHERE id=?2",
                    params![text, id],
                )?;
                transaction.commit()?;
                Ok((status, control))
            })
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn fail_run(
        &self,
        id: &RunId,
        code: crate::types::FailureCode,
        error: String,
    ) -> Result<(), String> {
        let id = id.clone();
        self.connection.call(move |db| -> Result<(), Error> {
            let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut run = load(&transaction, &id)?;
            if !run.status().is_open() { return Ok(()); }
            let requests = {
                let mut statement = transaction.prepare("SELECT data FROM requests WHERE run_id=? AND status NOT IN ('GREEN','RED','ERROR')")?;
                statement.query_map([&id], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>,_>>()?
            };
            for data in requests {
                let mut request: Request = serde_json::from_str(&data)?;
                request.state = super::RequestState::failed(error.clone(), Some(code), crate::broker::now());
                super::receipts::update_request(&transaction, &request)?;
            }
            run.state = super::RunState::Error { at: crate::broker::now(), error: Some(error) };
            if let Some(control) = &mut run.control { control.failure_code = Some(code); }
            save(&transaction, run)?;
            transaction.commit()?;
            Ok(())
        }).await.map_err(|e| e.to_string())
    }

    pub(crate) async fn admit(
        &self,
        id: &RunId,
        owner: crate::process::ChildIdentity,
    ) -> Result<(), String> {
        self.control(id, move |_, control| {
            let control = control.as_mut().ok_or("Run has no admission record.")?;
            if !matches!(control.worker, Worker::Starting { .. }) {
                return Err("Run was already admitted.".into());
            }
            control.worker = Worker::Admitted { owner };
            control.supervisor = Some(owner);
            Ok(())
        })
        .await
        .map(|_| ())
    }

    /// Ask the Run to stop; the saved status and control after the request.
    pub(crate) async fn request_cancel(
        &self,
        id: &RunId,
    ) -> Result<(RunStatus, Option<Control>), String> {
        self.control(id, |status, control| {
            if status.is_open() {
                let control = control.get_or_insert_with(Control::legacy);
                control.cancellation_requested = true;
                control.activity = Activity::Cancelling;
            }
            Ok(())
        })
        .await
    }
    /// Retire only this worker's executions; joined work belongs to its original Run. A
    /// sign-off still waiting on a retired execution, deferred or joined from another Run,
    /// could never be claimed again, so it ends with the execution.
    pub(crate) async fn retire_worker(
        &self,
        id: &RunId,
        code: crate::types::FailureCode,
    ) -> Result<(), String> {
        let id = id.clone();
        self.connection.call(move |db| -> Result<(), Error> {
            let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (_, control) = saved_control(&transaction, &id)?;
            if let Some(Control { worker: Worker::Admitted { owner }, .. }) = control {
                let executions = {
                    let mut statement = transaction.prepare("SELECT id FROM executions WHERE owner_pid=?1 AND owner_start_time=?2 AND status IN ('RUNNING','WAITING_HUMAN')")?;
                    statement.query_map(params![owner.pid, owner.start_time as i64], |row| row.get(0))?.collect::<Result<Vec<crate::types::ExecutionId>, _>>()?
                };
                for id in executions {
                    retire_execution(&transaction, &id, code, "Run worker stopped.")?;
                }
            }
            transaction.commit()?;
            Ok(())
        }).await.map_err(|e| e.to_string())
    }
}

/// End an active execution and every Human request still waiting on it.
pub(super) fn retire_execution(
    db: &rusqlite::Connection,
    id: &crate::types::ExecutionId,
    code: crate::types::FailureCode,
    error: &str,
) -> Result<(), Error> {
    let at = crate::broker::now();
    let waiting = {
        let mut statement = db.prepare("SELECT q.data FROM requests q JOIN executions e ON e.id=q.execution_id WHERE e.id=? AND e.status='WAITING_HUMAN' AND q.status IN ('WAITING_HUMAN','DEFERRED')")?;
        statement
            .query_map([id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    db.execute("UPDATE executions SET status='ERROR',data=json_set(data,'$.status','ERROR','$.error',?1,'$.errorCode',?2,'$.result',NULL,'$.completedAt',?3) WHERE id=?4 AND status IN ('RUNNING','WAITING_HUMAN')", params![error,code,at,id])?;
    for data in waiting {
        retire_request(db, serde_json::from_str(&data)?, code, error, at)?;
    }
    Ok(())
}

pub(super) fn retire_request(
    db: &rusqlite::Connection,
    mut request: Request,
    code: crate::types::FailureCode,
    error: &str,
    at: crate::types::Timestamp,
) -> Result<(), Error> {
    request.state = super::RequestState::failed(error.into(), Some(code), at);
    super::receipts::update_request(db, &request)
}

/// Compact observations don't decode saved graphs, owner payloads or execution logs.
pub(crate) struct Observation {
    pub status: RunStatus,
    pub control: Option<Control>,
    pub repo: std::path::PathBuf,
    pub created_at: crate::types::Timestamp,
    pub completed_at: Option<crate::types::Timestamp>,
    pub counts: std::collections::BTreeMap<crate::types::RequestStatus, u64>,
}

fn snapshot(state: &std::path::Path, id: &RunId) -> Result<Observation, Error> {
    let mut db = rusqlite::Connection::open_with_flags(
        state.join(super::DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    super::wait_when_busy(&db)?;
    let transaction = db.transaction()?;
    if !super::receipts::schema_initialized(&transaction)? {
        return Err(Error::Invalid("Run not found.".into()));
    }
    let (mut saved, control): (Observation, Option<String>) = transaction.query_row(
        "SELECT repo,status,json_extract(data,'$.control'),json_extract(data,'$.createdAt'),json_extract(data,'$.completedAt') FROM runs WHERE id=?", [id],
        |row| Ok((Observation {
            repo: std::path::PathBuf::from(row.get::<_, String>(0)?), status: row.get(1)?, control: None,
            created_at: row.get(3)?, completed_at: row.get(4)?, counts: Default::default(),
        }, row.get(2)?))).optional()?.ok_or_else(|| Error::Invalid("Run not found.".into()))?;
    crate::workspace::outside_workspace(&saved.repo, state)
        .map_err(|e| Error::Invalid(e.to_string()))?;
    saved.counts = {
        let mut statement = transaction
            .prepare("SELECT status,count(*) FROM requests WHERE run_id=? GROUP BY status")?;
        statement
            .query_map([id], |row| {
                Ok((
                    row.get(0)?,
                    u64::try_from(row.get::<_, i64>(1)?).expect("COUNT(*) is nonnegative"),
                ))
            })?
            .collect::<Result<_, _>>()?
    };
    saved.status = saved
        .status
        .with_signoffs(saved.counts.keys().any(|status| status.is_human_wait()));
    saved.control = control
        .map(|text| serde_json::from_str(&text))
        .transpose()?;
    transaction.commit()?;
    Ok(saved)
}

pub(crate) async fn observe(state: &std::path::Path, id: &RunId) -> Result<Observation, String> {
    let state = state.to_path_buf();
    let id = id.clone();
    crate::task::joined(
        tokio::task::spawn_blocking(move || {
            super::receipts::check_files(&state)?;
            snapshot(&state, &id).map_err(|e| e.to_string())
        })
        .await,
    )
    .map_err(|e| e.to_string())?
}

/// The repository a saved Run belongs to, read from its columns alone.
pub async fn read_run_repo(
    state: &std::path::Path,
    id: &RunId,
) -> Result<std::path::PathBuf, String> {
    let state = crate::workspace::canonical_target(state).map_err(|e| e.to_string())?;
    Ok(observe(&state, id).await?.repo)
}
