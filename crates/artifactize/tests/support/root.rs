//! Keep fixture inputs until every registered repository has released its Run owners.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Mutex,
};

enum Inputs {
    Temporary,
    Retained,
}

pub struct Root {
    directory: Option<tempfile::TempDir>,
    scopes: Mutex<BTreeSet<(PathBuf, PathBuf)>>,
    inputs: Inputs,
}

impl From<tempfile::TempDir> for Root {
    fn from(directory: tempfile::TempDir) -> Self {
        Self {
            directory: Some(directory),
            scopes: Mutex::default(),
            inputs: Inputs::Temporary,
        }
    }
}

impl Root {
    pub fn path(&self) -> &Path {
        self.directory
            .as_ref()
            .expect("fixture root is owned")
            .path()
    }

    /// Register before submission, so constructor errors and assertions cannot orphan a Run.
    pub fn track(&self, state: &Path, repo: &Path) {
        self.scopes
            .lock()
            .expect("register a fixture repository")
            .insert((state.to_owned(), repo.to_owned()));
    }

    /// Preserve a detached tool's cwd after its separate owner reports failed cleanup.
    /// Drop still releases every registered Run before retaining this directory.
    pub fn retain_inputs(&mut self) {
        self.inputs = Inputs::Retained;
    }

    /// Release before replacing lifecycle data with deliberately unsupported schema input.
    /// A failed release stays registered so Drop retains the fixture's files.
    pub fn release(&self, state: &Path, repo: &Path) -> Result<(), String> {
        super::release_repo_result(state, repo)?;
        self.scopes
            .lock()
            .expect("release a fixture repository")
            .remove(&(state.to_owned(), repo.to_owned()));
        Ok(())
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        // One failed repository must not detach owners in another registered scope.
        let errors: Vec<_> = self
            .scopes
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter_map(|(state, repo)| {
                super::release_repo_result(state, repo)
                    .err()
                    .map(|error| format!("{}: {error}", crate::support::os::path_text(repo)))
            })
            .collect();
        let directory = self.directory.take().expect("fixture root is owned");
        let message = if errors.is_empty() {
            match self.inputs {
                Inputs::Temporary => directory
                    .close()
                    .err()
                    .map(|error| format!("Fixture directory cleanup failed: {error}")),
                Inputs::Retained => {
                    let retained = directory.keep();
                    eprintln!(
                        "Fixture inputs retained at {}",
                        crate::support::os::path_text(&retained)
                    );
                    None
                }
            }
        } else {
            let retained = directory.keep();
            Some(format!(
                "Fixture Run cleanup failed; inputs retained at {}: {}",
                crate::support::os::path_text(&retained),
                errors.join("; ")
            ))
        };
        if let Some(message) = message {
            if std::thread::panicking() {
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        }
    }
}
