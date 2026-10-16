//! Keep fixture inputs until every registered repository has released its Run owners.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub struct Root {
    directory: Option<tempfile::TempDir>,
    scopes: Mutex<BTreeSet<(PathBuf, PathBuf)>>,
}

impl From<tempfile::TempDir> for Root {
    fn from(directory: tempfile::TempDir) -> Self {
        Self {
            directory: Some(directory),
            scopes: Mutex::default(),
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
            directory
                .close()
                .err()
                .map(|error| format!("Fixture directory cleanup failed: {error}"))
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
