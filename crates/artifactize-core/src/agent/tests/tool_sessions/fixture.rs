//! The fixture retains startup/close tasks through caller cancellation and unwinding.
use super::super::Fixture;
use crate::{
    config::read_workspace_config,
    tools::{
        Registry,
        sessions::{Sessions, executor},
    },
};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};
use tokio::{sync::oneshot, task::JoinHandle};
use tokio_util::sync::CancellationToken;

enum State {
    Running(JoinHandle<()>),
    Joined(Result<(), String>),
}
struct Task(tokio::sync::Mutex<State>);
impl Task {
    async fn join(&self) -> Result<(), String> {
        let mut state = self.0.lock().await;
        if let State::Running(task) = &mut *state {
            let result = task.await.map_err(|error| error.to_string());
            *state = State::Joined(result);
        }
        match &*state {
            State::Joined(result) => result.clone(),
            State::Running(_) => unreachable!("task joined"),
        }
    }
}

#[derive(Default)]
struct Owner {
    tasks: Mutex<Vec<Arc<Task>>>,
    cancellations: Mutex<Vec<CancellationToken>>,
}

impl Owner {
    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) -> Arc<Task> {
        let task = executor().expect("session fixture executor").spawn(future);
        let task = Arc::new(Task(tokio::sync::Mutex::new(State::Running(task))));
        self.tasks.lock().unwrap().push(task.clone());
        task
    }

    fn join(&self) -> Result<(), String> {
        loop {
            let tasks = std::mem::take(&mut *self.tasks.lock().unwrap());
            if tasks.is_empty() {
                return Ok(());
            }
            let executor = executor().map_err(|error| error.to_string())?.clone();
            // This waiter drives no child I/O. The library's independent session
            // executor keeps running while the calling test's runtime is blocked/gone.
            std::thread::spawn(move || {
                executor.block_on(async move {
                    let mut failures = Vec::new();
                    for task in tasks {
                        if let Err(error) = task.join().await {
                            failures.push(error.to_string());
                        }
                    }
                    if failures.is_empty() {
                        Ok(())
                    } else {
                        Err(failures.join("; "))
                    }
                })
            })
            .join()
            .map_err(|_| "session fixture join panicked".to_owned())??;
        }
    }
}

pub(super) struct OwnedFixture {
    fixture: Fixture,
    owner: Arc<Owner>,
}

impl std::ops::Deref for OwnedFixture {
    type Target = Fixture;
    fn deref(&self) -> &Fixture {
        &self.fixture
    }
}
impl std::ops::DerefMut for OwnedFixture {
    fn deref_mut(&mut self) -> &mut Fixture {
        &mut self.fixture
    }
}

impl OwnedFixture {
    pub fn new(fixture: Fixture) -> Self {
        Self {
            fixture,
            owner: Arc::default(),
        }
    }

    pub fn sessions(&self, cancellation: CancellationToken) -> OwnedSessions {
        self.owner
            .cancellations
            .lock()
            .unwrap()
            .push(cancellation.clone());
        let registry = Registry::new(&self.config, &self.config.evals[0].id).unwrap();
        OwnedSessions {
            sessions: Some(registry.sessions(&self.state(), None, cancellation)),
            owner: self.owner.clone(),
        }
    }

    pub fn ready_sessions(
        &self,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<OwnedSessions, String>> + Send>> {
        let config = read_workspace_config(&self.config.root).unwrap();
        let state = self.state();
        let output = self.output.clone();
        let owner = self.owner.clone();
        owner
            .cancellations
            .lock()
            .unwrap()
            .push(cancellation.clone());
        let (send, receive) = oneshot::channel();
        let startup_owner = owner.clone();
        // The bound probe and its close stay owned if the readiness waiter is dropped.
        let started = owner.spawn(async move {
            let result = match Registry::new(&config, &config.evals[0].id) {
                Ok(registry) => {
                    let sessions = registry.sessions(&state, None, cancellation.clone());
                    let result = registry
                        .with_session_scope(&sessions)
                        .unwrap()
                        .call("probe_a", serde_json::json!({}), &output, cancellation)
                        .await;
                    if let crate::tools::ToolResult::Error(error) = result {
                        let message = error.as_str().to_owned();
                        sessions.close().await.unwrap();
                        Err(message)
                    } else {
                        Ok(sessions)
                    }
                }
                Err(error) => Err(error),
            }
            .map(|sessions| OwnedSessions {
                sessions: Some(sessions),
                owner: startup_owner,
            });
            // A missing waiter drops OwnedSessions and schedules its owned close.
            let _ = send.send(result);
        });
        Box::pin(async move {
            started.join().await?;
            receive.await.map_err(|error| error.to_string())?
        })
    }
}

impl Drop for OwnedFixture {
    fn drop(&mut self) {
        for cancellation in self.owner.cancellations.lock().unwrap().iter() {
            cancellation.cancel();
        }
        // Stop a deliberately hanging fixture cooperatively during failed-test
        // teardown; the test itself still uses the explicit virtual expiry seam.
        let log = self.directory.path().join("log");
        let _ = std::fs::write(log.join("release"), "fixture teardown");
        let joined = self.owner.join();
        if joined.is_err() || !super::harness::directories(&self.state()).is_empty() {
            self.fixture.directory.disable_cleanup(true);
            let message = format!(
                "session fixture teardown did not finish: {joined:?}; retained {}",
                crate::platform::path_text(self.directory.path())
            );
            if std::thread::panicking() {
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        }
    }
}

pub(super) struct OwnedSessions {
    sessions: Option<Sessions>,
    owner: Arc<Owner>,
}
impl std::ops::Deref for OwnedSessions {
    type Target = Sessions;
    fn deref(&self) -> &Sessions {
        self.sessions.as_ref().expect("sessions not yet closed")
    }
}
impl OwnedSessions {
    fn schedule(&mut self) -> (oneshot::Receiver<Result<(), String>>, Arc<Task>) {
        let sessions = self.sessions.take().expect("sessions close once");
        let (send, receive) = oneshot::channel();
        let closed = self.owner.spawn(async move {
            let result = sessions.close().await;
            let _ = send.send(result);
        });
        (receive, closed)
    }
    pub async fn close(mut self) -> Result<(), String> {
        let (receive, closed) = self.schedule();
        // Join the task, not just its result notification: every captured lease
        // must have dropped before another startup attempts recovery.
        closed.join().await?;
        receive.await.map_err(|error| error.to_string())?
    }
}
impl Drop for OwnedSessions {
    fn drop(&mut self) {
        if self.sessions.is_some() {
            drop(self.schedule());
        }
    }
}
