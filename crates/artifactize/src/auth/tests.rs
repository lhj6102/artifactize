use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};

use super::storage::{Storage, Tokens};
use crate::test_os::{grant_everyone_read, symlink_dir, symlink_file};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Secret {
    token: String,
}

fn secret(token: &str) -> Secret {
    Secret {
        token: token.into(),
    }
}

fn test_storage(root: &Path) -> Storage {
    let directory = root.join("auth");
    crate::platform::create_private_dir(&directory).unwrap();
    Storage { directory }
}

/// Mode 0600 on Unix; on Windows a single-link file whose DACL grants only this user.
pub(crate) fn private_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().unwrap().permissions().mode() & 0o777 == 0o600
    }
    #[cfg(windows)]
    {
        crate::platform::is_private_file(&fs::File::open(path).unwrap()).unwrap()
    }
}

/// Mode 0700 on Unix; on Windows a DACL that grants only this user.
pub(crate) fn private_dir(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().unwrap().permissions().mode() & 0o777 == 0o700
    }
    #[cfg(windows)]
    {
        crate::platform::is_private_dir(path).unwrap()
    }
}

/// The inode on Unix, the file index on Windows: what a rename replaces.
fn file_id(file: &fs::File) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        file.metadata().unwrap().ino()
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: an open handle and a valid out pointer.
        assert_ne!(
            unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) },
            0
        );
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow)
    }
}

#[test]
fn storage_is_atomic_private_and_refuses_links() {
    let temp = tempfile::tempdir().unwrap();
    let storage = test_storage(temp.path());
    storage.save("secret.json", &secret("old-secret")).unwrap();
    let path = storage.directory.join("secret.json");
    let old = fs::File::open(&path).unwrap();
    let old_inode = file_id(&old);
    storage.save("secret.json", &secret("new-secret")).unwrap();
    assert_ne!(old_inode, file_id(&fs::File::open(&path).unwrap()));
    assert!(private_file(&path));
    assert!(private_dir(&storage.directory));
    assert_eq!(
        storage.read::<Secret>("secret.json").unwrap(),
        Some(secret("new-secret"))
    );
    assert_eq!(
        serde_json::from_reader::<_, Secret>(old).unwrap(),
        secret("old-secret")
    );
    assert_eq!(fs::read_dir(&storage.directory).unwrap().count(), 1);
    storage.remove("secret.json").unwrap();
    assert!(storage.read::<Secret>("secret.json").unwrap().is_none());
    storage.remove("secret.json").unwrap();

    let target = temp.path().join("public.json");
    fs::write(&target, r#"{"token":"public"}"#).unwrap();
    if symlink_file(&target, &path).is_some() {
        assert!(storage.read::<Secret>("secret.json").is_err());
        fs::remove_file(&path).unwrap();
    }
    fs::write(&path, r#"{"token":"readable"}"#).unwrap();
    grant_everyone_read(&path);
    assert!(storage.read::<Secret>("secret.json").is_err());
    fs::remove_file(&path).unwrap();
    // Another name for the file could outlive a rotation of this one.
    storage.save("secret.json", &secret("linked")).unwrap();
    fs::hard_link(&path, temp.path().join("second-link.json")).unwrap();
    assert!(storage.read::<Secret>("secret.json").is_err());
}

#[test]
fn credential_storage_rejects_repositories_and_symlink_escapes() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join(".git"), "worktree marker").unwrap();
    let Err(refusal) = Storage::new(Some(&repo.join("state")), None, Tokens::Remote) else {
        panic!("auth storage inside a git work tree");
    };
    assert!(refusal.starts_with("Remote token storage "), "{refusal}");
    assert!(
        refusal.contains("auth is inside the git work tree ")
            && refusal.ends_with("; artifactize keeps tokens outside repositories. Use a state directory outside it, or set ARTIFACTIZE_REMOTE_TOKEN."),
        "{refusal}"
    );
    let alias = temp.path().join("alias");
    if symlink_dir(&repo, &alias).is_some() {
        assert!(Storage::new(Some(&alias.join("state")), None, Tokens::Codex).is_err());
    }
    #[cfg(windows)]
    {
        let junction = temp.path().join("junction");
        crate::test_os::junction(&repo, &junction);
        assert!(Storage::new(Some(&junction.join("state")), None, Tokens::Codex).is_err());
    }
    fs::remove_file(repo.join(".git")).unwrap();
    let Err(refusal) = Storage::new(Some(&repo.join("state")), Some(&repo), Tokens::Codex) else {
        panic!("auth storage inside --repo");
    };
    assert!(
        refusal.starts_with("Codex sign-in storage ")
            && refusal.contains("auth is inside the reviewed repository ")
            && refusal.ends_with("or set ARTIFACTIZE_CODEX_AUTH_FILE."),
        "{refusal}"
    );
    fs::write(repo.join("artifactize.json"), "{}").unwrap();
    let Err(refusal) = Storage::new(Some(&repo.join("state")), None, Tokens::Codex) else {
        panic!("auth storage inside an artifactize workspace");
    };
    assert!(
        refusal.contains("auth is inside the artifactize workspace "),
        "{refusal}"
    );
    assert!(!repo.join("state").exists());
    let state = temp.path().join("state");
    let storage = Storage::new(Some(&state), None, Tokens::Codex).unwrap();
    assert!(private_dir(&storage.directory));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&storage.directory, fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(windows)]
    grant_everyone_read(&storage.directory);
    assert!(Storage::new(Some(&state), None, Tokens::Codex).is_err());
}

#[tokio::test]
async fn named_locks_serialize_holders_and_stay_private() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Arc::new(test_storage(temp.path()));
    let first = storage.lock("codex").await.unwrap();
    let lock = storage.directory.join("codex.lock");
    assert!(private_file(&lock));
    // Another name is independent of a held lock.
    drop(storage.lock("other").await.unwrap());
    let acquired = Arc::new(AtomicBool::new(false));
    let waiter = tokio::spawn({
        let storage = storage.clone();
        let acquired = acquired.clone();
        async move {
            let _second = storage.lock("codex").await.unwrap();
            acquired.store(true, Ordering::SeqCst);
        }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!acquired.load(Ordering::SeqCst));
    drop(first);
    tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .unwrap()
        .unwrap();
    assert!(acquired.load(Ordering::SeqCst));
    fs::remove_file(&lock).unwrap();
    if symlink_file(temp.path().join("elsewhere"), &lock).is_some() {
        assert!(storage.lock("codex").await.is_err());
    }
}
