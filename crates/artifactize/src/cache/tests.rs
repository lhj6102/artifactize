use std::{fs, path::Path};

use serde_json::{Value, json};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::config::{CONFIG_FILE, read_workspace_config};
use crate::test_os::symlink_file;

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

    async fn fingerprint(&self, id: &str) -> Result<PreparedFingerprint, String> {
        let config = read_workspace_config(self.root.path()).map_err(|e| e.to_string())?;
        let mut fingerprints = prepare(
            &config,
            [id],
            self.output.path(),
            &Parallelism::new(2),
            CancellationToken::new(),
        )
        .await?;
        Ok(fingerprints.remove(id).unwrap())
    }

    async fn files(&self, id: &str) -> Vec<String> {
        let fingerprint = self.fingerprint(id).await.unwrap();
        fingerprint
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
        json!({"name":"root","fingerprint":{"ignore":["*.log","/data/raw"]}}),
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
    let before = repo.fingerprint("root").await.unwrap().value;
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
        assert_eq!(
            repo.fingerprint("root").await.unwrap().value,
            before,
            "{path}"
        );
    }
    repo.write("sub/keep.tmp", "v2");
    assert_ne!(repo.fingerprint("root").await.unwrap().value, before);
}

#[tokio::test]
async fn gitignores_apply_from_the_repository_root_down_with_git_precedence() {
    let repo = Repo::new();
    repo.artifact(
        "pkg/app",
        json!({"name":"app","fingerprint":{"files":["src"]}}),
    );
    repo.write(".gitignore", "*.cache\nbuild/\n/pkg/app/src/anchored.txt\n");
    repo.write("pkg/.gitignore", "!keep.cache\n/app/src/relative.txt\n");
    repo.write("pkg/app/.gitignore", "local.txt\n");
    for path in [
        "a.txt",
        "x.cache",
        "keep.cache",
        "anchored.txt",
        "relative.txt",
        "local.txt",
        "build/out",
        "sub/anchored.txt",
        "sub/relative.txt",
    ] {
        repo.write(&format!("pkg/app/src/{path}"), "v1");
    }
    assert_eq!(
        repo.files("app").await,
        [
            "src/a.txt",
            "src/keep.cache",
            "src/sub/anchored.txt",
            "src/sub/relative.txt"
        ]
    );
    let before = repo.fingerprint("app").await.unwrap().value;
    repo.write("pkg/app/src/__pycache__/m.pyc", "generated");
    repo.write("pkg/app/src/other.cache", "generated");
    assert_eq!(repo.fingerprint("app").await.unwrap().value, before);
}

#[tokio::test]
async fn content_inputs_reject_links_unless_ignored_and_name_paths_inside_the_owner() {
    let repo = Repo::new();
    repo.artifact("owner", json!({"name":"owner","fingerprint":{}}));
    repo.write("owner/a.txt", "a");
    let link = repo.root.path().join("owner/link");
    let check = |error: String| {
        assert!(
            error.contains("Content fingerprint for Artifact owner failed: link:"),
            "{error}"
        );
        assert!(error.contains("fingerprint.ignore"), "{error}");
    };
    if symlink_file("a.txt", &link).is_some() {
        check(repo.fingerprint("owner").await.unwrap_err());
    }
    // A junction is a link too, and needs no privilege to create.
    #[cfg(windows)]
    {
        let _ = fs::remove_file(&link);
        repo.write("elsewhere/b.txt", "b");
        crate::test_os::junction(&repo.root.path().join("elsewhere"), &link);
        check(repo.fingerprint("owner").await.unwrap_err());
    }
    repo.artifact(
        "owner",
        json!({"name":"owner","fingerprint":{"files":["a.txt","docs"],"ignore":["link"]}}),
    );
    let error = repo.fingerprint("owner").await.unwrap_err();
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
            json!({"name":"owner","fingerprint":{"files":inputs}}),
        );
        assert!(read_workspace_config(repo.root.path()).is_err(), "{inputs}");
    }
    for ignore in [json!(["!keep"]), json!([""]), json!(["#x"])] {
        repo.artifact(
            "owner",
            json!({"name":"owner","fingerprint":{"ignore":ignore}}),
        );
        assert!(read_workspace_config(repo.root.path()).is_err(), "{ignore}");
    }
}

#[tokio::test]
async fn family_instances_hash_shared_files_and_only_their_own_material() {
    let repo = Repo::new();
    repo.artifact(
        "posts",
        json!({"name":"posts","family":{"instances":"instances.json"},"fingerprint":{}}),
    );
    repo.write(
        "posts/instances.json",
        &json!({"one":{"material":["one.md"]},"two":{"material":["two.md"]}}).to_string(),
    );
    for path in ["posts/check.py", "posts/one.md", "posts/two.md"] {
        repo.write(path, "v1");
    }
    assert_eq!(repo.files("one").await, ["check.py", "one.md"]);
    let one = repo.fingerprint("one").await.unwrap().value;
    repo.write("posts/two.md", "v2");
    assert_eq!(repo.fingerprint("one").await.unwrap().value, one);
    repo.write("posts/check.py", "v2");
    assert_ne!(repo.fingerprint("one").await.unwrap().value, one);
}

#[test]
fn an_eval_depends_on_its_target_mounts_children_and_named_artifacts_only() {
    let repo = Repo::new();
    repo.artifact(
        "a",
        json!({"name":"a","mounts":{"next":"b"},"evals":[
            {"id":"plain","title":"Plain","profile":{"kind":"human"},"payload":{"instruction":"Review."}},
            {"id":"named","title":"Named","profile":{"kind":"runtime","command":"cat","args":["{d}/x"]},
                "payload":{"instruction":"Compare with {e}."}}
        ]}),
    );
    repo.artifact("b", json!({"name":"b","mounts":{"next":"c"}}));
    repo.artifact("c", json!({"name":"c","mounts":{"next":"a"}}));
    repo.artifact("a/child", json!({"name":"child","mounts":{"far":"c"}}));
    repo.artifact("d", json!({"name":"d"}));
    repo.write("d/x", "x");
    repo.artifact("e", json!({"name":"e","mounts":{"far":"c"}}));
    let config = read_workspace_config(repo.root.path()).unwrap();
    let names = |id: &str| {
        let eval = config.evals.iter().find(|eval| eval.id == id).unwrap();
        dependencies(&config, eval).into_iter().collect::<Vec<_>>()
    };
    // Two connections away (b's mount c, child's mount c) is never followed.
    assert_eq!(names("a/plain"), ["a", "b", "child"]);
    assert_eq!(names("a/named"), ["a", "b", "child", "d", "e"]);
}

#[test]
fn the_key_covers_the_strategy_and_each_named_fingerprint_but_no_execution_option() {
    let eval = |profile: Value, extra: Value| -> EvalDeclaration {
        let mut declaration = json!({"id":"check","title":"Check","profile":profile,
            "payload":{"instruction":"Review."}});
        declaration
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value(declaration).unwrap()
    };
    let agent = |model: &str, extra: Value| {
        let mut profile = json!({"kind":"agent","backend":"openai","model":model});
        profile
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        profile
    };
    let base = eval_definition_hash(&eval(agent("a", json!({})), json!({})));
    for profile in [
        agent("b", json!({})),
        agent("a", json!({"reasoning":"high"})),
        agent("a", json!({"timeoutMs":5})),
        agent("a", json!({"maxToolCalls":5,"maxTokens":5})),
    ] {
        assert_eq!(eval_definition_hash(&eval(profile, json!({}))), base);
    }
    assert_eq!(
        eval_definition_hash(&eval(
            agent("a", json!({})),
            json!({"id":"other","title":"Other","profileVariants":{"fast":agent("b", json!({}))}})
        )),
        base
    );
    for changed in [
        eval(
            agent("a", json!({})),
            json!({"payload":{"instruction":"Other."}}),
        ),
        eval(
            agent("a", json!({})),
            json!({"passSchema":{"type":"object"}}),
        ),
        eval(
            agent("a", json!({})),
            json!({"failSchema":{"type":"object"}}),
        ),
        eval(json!({"kind":"human"}), json!({})),
    ] {
        assert_ne!(eval_definition_hash(&changed), base);
    }
    let runtime = |args: Value, timeout: Value| {
        let mut profile = json!({"kind":"runtime","command":"check","args":args});
        if !timeout.is_null() {
            profile["timeoutMs"] = timeout;
        }
        eval_definition_hash(&eval(profile, json!({})))
    };
    assert_eq!(
        runtime(json!(["a"]), json!(null)),
        runtime(json!(["a"]), json!(9))
    );
    assert_ne!(
        runtime(json!(["a"]), json!(null)),
        runtime(json!(["b"]), json!(null))
    );

    let fingerprints = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    };
    let one = key(&base, &fingerprints(&[("app", "v1"), ("core", "v1")]));
    assert_eq!(one.len(), 64);
    assert_eq!(
        key(&base, &fingerprints(&[("core", "v1"), ("app", "v1")])),
        one
    );
    for other in [
        key(&base, &fingerprints(&[("app", "v1"), ("core", "v2")])),
        key(&base, &fingerprints(&[("app", "v1")])),
        key(&base, &fingerprints(&[("api", "v1"), ("core", "v1")])),
        key(
            &eval_definition_hash(&eval(json!({"kind":"human"}), json!({}))),
            &fingerprints(&[("app", "v1"), ("core", "v1")]),
        ),
    ] {
        assert_ne!(other, one);
    }
}

#[test]
fn changes_name_target_files_and_dependency_fingerprints() {
    let manifest = |files: &[(&str, &str)]| Manifest {
        inputs: format!("{files:?}"),
        files: Some(
            files
                .iter()
                .map(|(path, digest)| (path.to_string(), digest.to_string()))
                .collect(),
        ),
    };
    let fingerprints = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    };
    let execution = |manifest: Option<Manifest>,
                     fingerprints: BTreeMap<String, String>|
     -> Execution {
        serde_json::from_value(json!({
            "id":"execution-1","key":"old","fingerprint":"old","fingerprints":fingerprints,
            "evalDefHash":"hash","ownerPid":1,"ownerStartTime":1,
            "status":"GREEN","result":null,"error":null,"errorCode":null,"profile":null,"usage":null,
            "provenance":{"repoPath":"/repo","runId":"run-1","requestId":"run-1-1","evalId":"a/check","evalDefHash":"hash","completedAt":null},
            "startedAt":"now","completedAt":"now","manifest":manifest
        }))
        .unwrap()
    };
    let current = |pairs: &[(&str, &str)]| Key {
        value: "new".into(),
        eval_def_hash: "hash".into(),
        fingerprints: fingerprints(pairs),
    };
    let old = manifest(&[("src/a.py", "1"), ("old.md", "1"), ("same", "1")]);
    let new = manifest(&[("src/a.py", "2"), ("docs/new.md", "1"), ("same", "1")]);
    let before = fingerprints(&[("a", "content:1"), ("core", "x"), ("gone", "x")]);
    let after = current(&[("a", "content:2"), ("core", "y"), ("extra", "x")]);
    let changes = changes(
        &execution(Some(old.clone()), before.clone()),
        "a",
        &after,
        Some(&new),
    );
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
    // Only a dependency changed: the target's files are not listed.
    let only = current(&[("a", "content:1"), ("core", "y"), ("gone", "x")]);
    let changes = super::changes(
        &execution(Some(old.clone()), before.clone()),
        "a",
        &only,
        Some(&old),
    );
    assert!(changes.files.is_none());
    assert_eq!(changes.summary, "dependency core changed");
    let mut bounded = old;
    bounded.files = None;
    assert_eq!(
        super::changes(
            &execution(Some(bounded), before.clone()),
            "a",
            &after,
            Some(&new)
        )
        .summary,
        "inputs changed; dependency core changed; dependency extra added; dependency gone removed"
    );
    let script = current(&[("a", "v2")]);
    let changes = super::changes(
        &execution(None, fingerprints(&[("a", "v1")])),
        "a",
        &script,
        None,
    );
    assert_eq!(changes.summary, "fingerprint changed");
    assert!(changes.files.is_none() && changes.dependencies.is_none());
}

/// The keys of evals without a `resultCheck`, computed by artifactize 0.5 before 0.6.0
/// removed it: they do not change, so their results stay reusable.
#[test]
fn keys_of_evals_without_a_result_check_are_pinned() {
    let agent: EvalDeclaration = serde_json::from_value(json!({
        "id":"spec-coverage","title":"Spec coverage",
        "profile":{"kind":"agent","backend":"openai","model":"gpt-5.1","reasoning":"high","timeoutMs":60000,"maxToolCalls":20},
        "payload":{"instruction":"Check that {spec} covers every requirement.","focus":["errors","limits"]},
        "passSchema":{"type":"object","properties":{"summary":{"type":"string"}},"required":["summary"]},
        "failSchema":{"type":"object","properties":{"missing":{"type":"array","items":{"type":"string"}}},"required":["missing"]}
    }))
    .unwrap();
    let runtime: EvalDeclaration = serde_json::from_value(json!({
        "id":"tests","title":"Tests",
        "profile":{"kind":"runtime","command":"./check.sh","args":["{spec}/rules.md","--strict"],"timeoutMs":9000},
        "payload":{"instruction":"Run the checks."}
    }))
    .unwrap();
    let fingerprints: BTreeMap<String, String> = [("app", "content:1111"), ("spec", "script-v2")]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    for (eval, hash, pinned) in [
        (
            &agent,
            "65116b8f0d4f8bcb1d8a197706fa57d4a1881216edb6e8533086a6deb2b7abb4",
            "f79bfb7fa26e3393235264152e0310e95ce48fb5de0842d8b58ebd3dd431fc13",
        ),
        (
            &runtime,
            "bbc05cdeb642f96bdce73eaea01a2a7273e12d6e0c2642ef4bc2f3b61959cd1d",
            "69fb9f32d95c97bc0c5c471e98644da7093337a89d553e383159402c1eb57f5e",
        ),
    ] {
        assert_eq!(eval_definition_hash(eval), hash);
        assert_eq!(key(hash, &fingerprints), pinned);
    }
}
