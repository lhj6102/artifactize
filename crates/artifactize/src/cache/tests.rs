use std::{fs, os::unix::fs::symlink, path::Path};

use serde_json::{Value, json};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::config::{CONFIG_FILE, Dependencies, read_workspace_config};

struct Repo {
    root: TempDir,
    output: TempDir,
}

impl Repo {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            output: tempfile::tempdir().unwrap(),
        }
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn artifact(&self, folder: &str, value: Value) {
        let path = Path::new(folder).join(CONFIG_FILE);
        self.write(path.to_str().unwrap(), &value.to_string());
    }

    async fn identity(&self, id: &str) -> Result<Identity, String> {
        let config = read_workspace_config(self.root.path()).map_err(|e| e.to_string())?;
        let mut identities =
            prepare(&config, [id], self.output.path(), CancellationToken::new()).await?;
        Ok(identities.remove(id).unwrap())
    }

    async fn files(&self, id: &str) -> Vec<String> {
        let identity = self.identity(id).await.unwrap();
        identity
            .manifest
            .unwrap()
            .files
            .unwrap()
            .into_keys()
            .collect()
    }
}

#[tokio::test]
async fn content_skips_generated_ignored_child_and_declaration_files() {
    let repo = Repo::new();
    repo.artifact(
        "",
        json!({"name":"root","stale":{"kind":"content","dependencies":"none","ignore":["*.log","/data/raw"]}}),
    );
    for path in [
        "a.txt",
        "notes.log",
        "data/raw",
        "data/kept",
        "build/out.bin",
        "sub/keep.tmp",
        "sub/drop.tmp",
        "__pycache__/m.cpython-312.pyc",
        "lib.pyc",
        "node_modules/x/index.js",
        "target/debug/app",
        ".git",
        "child/c.txt",
    ] {
        repo.write(path, "v1");
    }
    repo.write(".gitignore", "# generated\nbuild/\n*.tmp\n");
    repo.write("sub/.gitignore", "!keep.tmp\n");
    repo.artifact("child", json!({"name":"child"}));
    assert_eq!(
        repo.files("root").await,
        [
            ".gitignore",
            "a.txt",
            "data/kept",
            "sub/.gitignore",
            "sub/keep.tmp"
        ]
    );
    let before = repo.identity("root").await.unwrap().value;
    assert!(before.starts_with("content:") && before.len() == 72);
    for path in [
        "notes.log",
        "data/raw",
        "build/out.bin",
        "sub/drop.tmp",
        "__pycache__/m.cpython-312.pyc",
        "lib.pyc",
        "node_modules/x/index.js",
        "target/debug/app",
        ".git",
        "child/c.txt",
        "build/new/file",
    ] {
        repo.write(path, "v2");
        assert_eq!(repo.identity("root").await.unwrap().value, before, "{path}");
    }
    repo.write("sub/keep.tmp", "v2");
    assert_ne!(repo.identity("root").await.unwrap().value, before);
}

#[tokio::test]
async fn content_inputs_reject_links_unless_ignored_and_name_paths_inside_the_owner() {
    let repo = Repo::new();
    repo.artifact("owner", json!({"name":"owner","stale":{"kind":"content"}}));
    repo.write("owner/a.txt", "a");
    symlink("a.txt", repo.root.path().join("owner/link")).unwrap();
    let error = repo.identity("owner").await.unwrap_err();
    assert!(
        error.contains("Content identity for Artifact owner failed: link:"),
        "{error}"
    );
    assert!(error.contains("stale.ignore"), "{error}");
    repo.artifact(
        "owner",
        json!({"name":"owner","stale":{"kind":"content","inputs":["a.txt","docs"],"ignore":["link"]}}),
    );
    let error = repo.identity("owner").await.unwrap_err();
    assert!(error.contains("docs:"), "{error}");
    repo.write("owner/docs/guide.md", "guide");
    assert_eq!(repo.files("owner").await, ["a.txt", "docs/guide.md"]);
    repo.artifact("owner/docs/nested", json!({"name":"nested"}));
    for inputs in [
        json!(["docs/nested"]),
        json!(["../x"]),
        json!([]),
        json!([".", "."]),
    ] {
        repo.artifact(
            "owner",
            json!({"name":"owner","stale":{"kind":"content","inputs":inputs}}),
        );
        assert!(read_workspace_config(repo.root.path()).is_err(), "{inputs}");
    }
    for ignore in [json!(["!keep"]), json!([""]), json!(["#x"])] {
        repo.artifact(
            "owner",
            json!({"name":"owner","stale":{"kind":"content","ignore":ignore}}),
        );
        assert!(read_workspace_config(repo.root.path()).is_err(), "{ignore}");
    }
}

#[tokio::test]
async fn family_instances_hash_shared_files_and_only_their_own_material() {
    let repo = Repo::new();
    repo.artifact(
        "posts",
        json!({"name":"posts","family":{"instances":"instances.json"},"stale":{"kind":"content"}}),
    );
    repo.write(
        "posts/instances.json",
        &json!({"one":{"material":["one.md"]},"two":{"material":["two.md"]}}).to_string(),
    );
    for path in ["posts/check.py", "posts/one.md", "posts/two.md"] {
        repo.write(path, "v1");
    }
    assert_eq!(repo.files("one").await, ["check.py", "one.md"]);
    let one = repo.identity("one").await.unwrap().value;
    repo.write("posts/two.md", "v2");
    assert_eq!(repo.identity("one").await.unwrap().value, one);
    repo.write("posts/check.py", "v2");
    assert_ne!(repo.identity("one").await.unwrap().value, one);
}

#[test]
fn dependency_scopes_terminate_on_cycles_and_exclude_the_owner() {
    let repo = Repo::new();
    repo.artifact("a", json!({"name":"a","mounts":{"next":"b"}}));
    repo.artifact("b", json!({"name":"b","mounts":{"next":"c"}}));
    repo.artifact("c", json!({"name":"c","mounts":{"next":"a"}}));
    repo.artifact("a/child", json!({"name":"child"}));
    let config = read_workspace_config(repo.root.path()).unwrap();
    let scope = |scope| {
        content::dependencies(&config, "a", scope)
            .into_iter()
            .collect::<Vec<_>>()
    };
    assert!(scope(Dependencies::None).is_empty());
    assert_eq!(scope(Dependencies::Direct), ["b", "child"]);
    assert_eq!(scope(Dependencies::Transitive), ["b", "c", "child"]);
}

#[test]
fn changes_list_files_and_dependencies_or_fall_back_to_the_identity() {
    let manifest = |files: &[(&str, &str)], dependencies: &[(&str, &str)]| Manifest {
        inputs: format!("{files:?}"),
        files: Some(
            files
                .iter()
                .map(|(path, digest)| (path.to_string(), digest.to_string()))
                .collect(),
        ),
        dependencies: Some(
            dependencies
                .iter()
                .map(|(id, value)| (id.to_string(), value.to_string()))
                .collect(),
        ),
    };
    let execution = |manifest: Option<Manifest>| -> Execution {
        serde_json::from_value(json!({
            "id":"execution-1","identity":"old","evalDefHash":"hash","ownerPid":1,"ownerStartTime":1,
            "status":"GREEN","result":null,"error":null,"errorCode":null,"profile":null,"usage":null,
            "provenance":{"repoPath":"/repo","runId":"run-1","requestId":"run-1-1","evalId":"a/check","evalDefHash":"hash","completedAt":null},
            "startedAt":"now","completedAt":"now","manifest":manifest
        }))
        .unwrap()
    };
    let old = manifest(
        &[("src/a.py", "1"), ("old.md", "1"), ("same", "1")],
        &[("core", "x"), ("gone", "x")],
    );
    let new = Identity {
        value: "new".into(),
        manifest: Some(manifest(
            &[("src/a.py", "2"), ("docs/new.md", "1"), ("same", "1")],
            &[("core", "y"), ("extra", "x")],
        )),
    };
    let changes = changes(&execution(Some(old.clone())), &new);
    assert_eq!(changes.since_run_id, "run-1");
    assert_eq!(
        changes.files.unwrap(),
        ["+docs/new.md", "-old.md", "src/a.py"]
    );
    assert_eq!(changes.dependencies.unwrap(), ["core", "+extra", "-gone"]);
    assert_eq!(
        changes.summary,
        "changed: +docs/new.md, -old.md, src/a.py; dependency core changed; dependency extra added; dependency gone removed"
    );
    let mut bounded = old;
    bounded.files = None;
    bounded.dependencies = None;
    assert_eq!(
        super::changes(&execution(Some(bounded)), &new).summary,
        "inputs changed"
    );
    let script = Identity {
        value: "new".into(),
        manifest: None,
    };
    let changes = super::changes(&execution(None), &script);
    assert_eq!(changes.summary, "identity changed");
    assert!(changes.files.is_none() && changes.dependencies.is_none());
}
