//! Run admission and cancellation share the Run row's write lock with scheduler saves.
use super::{Receipts, Request, Run, receipts::Error};
use crate::{
    runs::{Activity, Control, Worker},
    types::{RequestKey, RunId, RunStatus},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

pub(super) fn load(db: &rusqlite::Connection, id: &RunId) -> Result<Run, Error> {
    let data: String = db.query_row("SELECT data FROM runs WHERE id=?", [id], |row| row.get(0))?;
    Ok(serde_json::from_str(&data)?)
}

/// Never replace concurrent admission/cancellation with an older scheduler clone.
pub(super) fn save(db: &rusqlite::Connection, mut run: Run) -> Result<(), Error> {
    let current = load(db, &run.id)?;
    if current.status() != RunStatus::Running && current.control.is_some() {
        return Ok(());
    }
    let running = run.status() == RunStatus::Running;
    if let (Some(control), Some(saved)) = (&mut run.control, current.control) {
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
    db.execute(
        "UPDATE runs SET status=?,data=? WHERE id=?",
        params![run.status(), serde_json::to_string(&run)?, run.id],
    )?;
    Ok(())
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
            transaction.execute("INSERT INTO runs(id,repo,status,data,request_key) VALUES (?,?,?,?,?)", params![run.id,repo,run.status(),serde_json::to_string(&run)?,run.request_key])?;
            for (ordinal, request) in requests.iter().enumerate() {
                transaction.execute("INSERT INTO requests(id,run_id,eval_id,ordinal,status,data) VALUES (?,?,?,?,?,?)", params![request.id,run.id,request.eval_id,ordinal as i64,request.status(),serde_json::to_string(request)?])?;
            }
            transaction.commit()?;
            Ok(run.id)
        }).await.map_err(|error| error.to_string())
    }

    pub(crate) async fn control(
        &self,
        id: &RunId,
        change: impl FnOnce(&mut Run) -> Result<(), String> + Send + 'static,
    ) -> Result<Run, String> {
        let id = id.clone();
        self.connection
            .call(move |db| -> Result<Run, Error> {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let mut run = load(&transaction, &id)?;
                change(&mut run).map_err(Error::Invalid)?;
                transaction.execute(
                    "UPDATE runs SET status=?,data=? WHERE id=?",
                    params![run.status(), serde_json::to_string(&run)?, id],
                )?;
                transaction.commit()?;
                Ok(run)
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
            if run.status() != RunStatus::Running { return Ok(()); }
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
        self.control(id, move |run| {
            let control = run.control.as_mut().ok_or("Run has no admission record.")?;
            if !matches!(control.worker, Worker::Starting { .. }) {
                return Err("Run was already admitted.".into());
            }
            control.worker = Worker::Admitted { owner };
            Ok(())
        })
        .await
        .map(|_| ())
    }

    pub(crate) async fn request_cancel(&self, id: &RunId) -> Result<Run, String> {
        self.control(id, |run| {
            if run.status() == RunStatus::Running {
                let control = run.control.get_or_insert_with(Control::legacy);
                control.cancellation_requested = true;
                control.activity = Activity::Cancelling;
            }
            Ok(())
        })
        .await
    }
    /// Retire only this worker's executions; joined work belongs to its original Run.
    pub(crate) async fn retire_worker(
        &self,
        id: &RunId,
        code: crate::types::FailureCode,
    ) -> Result<(), String> {
        let id = id.clone();
        self.connection.call(move |db| -> Result<(), Error> {
            let run = load(db, &id)?;
            if let Some(Control { worker: Worker::Admitted { owner }, .. }) = run.control {
                db.execute("UPDATE executions SET status='ERROR',data=json_set(data,'$.status','ERROR','$.error','Run worker stopped.','$.errorCode',?1,'$.result',NULL,'$.completedAt',?2) WHERE owner_pid=?3 AND owner_start_time=?4 AND status IN ('RUNNING','WAITING_HUMAN')", params![code,crate::broker::now(),owner.pid,owner.start_time as i64])?;
            }
            Ok(())
        }).await.map_err(|e| e.to_string())
    }
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
    db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
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
