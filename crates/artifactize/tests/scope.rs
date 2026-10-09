use std::fs;
use std::path::PathBuf;

use artifactize::config::{Profile, read_workspace_config};
use artifactize::runtime::{self, Command, Outcome, Verdict};
use artifactize::scope::{eval_scope, resolve_argv, scoped_path};
use serde_json::json;
use support::os::bin;
use tokio_util::sync::CancellationToken;

mod support;

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn resolved_child_mount_and_global_inputs_reach_the_runtime_executor() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("repo");
    fs::create_dir_all(root.join("review/nested")).unwrap();
    fs::create_dir_all(root.join("data")).unwrap();
    let fixture = Fixture(support::os::canonical(&root));
    let root = &fixture.0;
    support::declaration::write(
        root.join("review/index.artf"),
        json!({
            "name":"review",
            "mounts":{"source":"input"},
            "evals":[
                {
                    "id":"read",
                    "title":"Read inputs",
                    "payload":{"instruction":"Inspect inputs."},
                    "profile":{
                        "kind":"runtime",
                        "command":bin("/bin/cat"),
                        "args":[
                            "local",
                            "{review}/nested/file",
                            "{review}/source/file",
                            "{input}/file",
                        ],
                    },
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
    fs::write(root.join("review/local"), "owner\n").unwrap();
    support::declaration::write(root.join("review/nested/index.artf"), r#"{"name":"child"}"#)
        .unwrap();
    fs::write(root.join("review/nested/file"), "child\n").unwrap();
    support::declaration::write(root.join("data/index.artf"), r#"{"name":"input"}"#).unwrap();
    fs::write(root.join("data/file"), "input\n").unwrap();

    let config = read_workspace_config(root).unwrap();
    let eval = &config.evals[0];
    let scope = eval_scope(&config, eval).unwrap();
    let Profile::Runtime { command, args, .. } = &eval.declaration.profile else {
        panic!()
    };
    let args = resolve_argv(&config, &scope, &eval.target, args).unwrap();
    let cwd = scoped_path(root, &config.artifacts[&eval.target].path).unwrap();
    let output = Fixture(scratch.path().join("output"));
    let mut command = Command::prepare(
        command.into(),
        args.into_iter().map(Into::into).collect(),
        root,
        &output.0,
        Some(std::time::Duration::from_millis(5000)),
    )
    .unwrap();
    command.cwd = cwd;
    let outcome = runtime::execute(command, CancellationToken::new(), |_| async { Ok(()) }).await;
    let Outcome::Completed(result) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(result.verdict, Verdict::Green);
    assert_eq!(result.output.stdout, b"owner\nchild\ninput\ninput\n");
}
