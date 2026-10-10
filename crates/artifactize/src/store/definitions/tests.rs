//! Independent schema-5 wire fixtures pin graph capture and hash representation.
use super::*;
use crate::project::selection::Selection;
use serde_json::json;
use sha2::{Digest, Sha256};

#[test]
fn saved_graph_keeps_known_null_missing_and_extensible_unknown_fields() {
    let mut wire = json!({
        "version":1,
        "repoPath":"/saved/workspace",
        "selection":{"kind":"all"},
        "futureTop":{"v":7},
        "artifacts":{
            "page":{
                "path":"pages",
                "name":"page",
                "children":{},
                "mounts":{},
                "basis":null,
                "fingerprint":{
                    "script":{
                        "command":"echo",
                        "args":["v1"],
                        "files":[],
                        "timeoutMs":null,
                        "futureScript":true,
                    },
                },
                "reviewPolicy":{"dependencyGates":null,"futurePolicy":1},
                "views":{
                    "agentTools":{"read":{"builtin":"read","futureTool":true}},
                    "humanTools":{
                        "inspect":{
                            "description":"Inspect",
                            "kind":"output",
                            "command":"cat",
                            "args":["file.md"],
                            "futureHumanTool":1,
                        },
                    },
                    "futureView":[],
                },
                "futureArtifact":"kept",
            },
        },
        "evals":[
            {
                "id":"page/check",
                "target":"page",
                "references":{},
                "deps":["input"],
                "declaration":{
                    "id":"check",
                    "title":"Inspect",
                    "profile":{"kind":"runtime","command":"true","args":[],"timeoutMs":null},
                    "profileVariants":{},
                    "payload":{"instruction":"Inspect {input}.","owner":[true,null,42]},
                    "passSchema":{"type":"object","properties":{"accepted":{"const":true}}},
                    "failSchema":null,
                    "futureDeclaration":true,
                },
                "futureEval":2,
            },
        ],
        "relations":[
            {
                "source":"input",
                "target":"page",
                "kind":"instruction",
                "evalId":"page/check",
                "name":"input",
                "cyclic":true,
                "futureRelation":false,
            },
        ],
        "components":[
            {
                "id":0,
                "artifacts":["input","page"],
                "dependencies":[],
                "gates":["input/check"],
                "cyclic":true,
                "futureComponent":3,
            },
        ],
    });
    wire["selection"]["futureSelection"] = json!({"v":true});
    wire["evals"][0]["declaration"]["profile"]["futureProfile"] = json!({"v":1});
    wire["evals"][0]["declaration"]["profileVariants"] =
        json!({"extra":{"kind":"human","futureVariant":[1,2]}});
    wire["relations"].as_array_mut().unwrap().push(
        json!({"source":"page","target":"input","kind":"future-edge","note":{"future":true}}),
    );
    let snapshot: Definitions = serde_json::from_value(wire.clone()).unwrap();
    let restored = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(restored, wire);
    assert_eq!(
        Sha256::digest(serde_json::to_vec(&restored).unwrap()),
        Sha256::digest(serde_json::to_vec(&wire).unwrap())
    );
    let graph = snapshot.graph().unwrap();
    assert_eq!(
        graph.repo_path.value().unwrap(),
        &PathBuf::from("/saved/workspace")
    );
    assert_eq!(
        graph.components()[0].artifacts.value().unwrap(),
        &["input", "page"]
    );
    assert!(matches!(graph.artifact("page").unwrap().basis, Field::Null));
    for wire in [
        Value::Null,
        json!({}),
        json!({"artifacts":null,"evals":null}),
        json!({"artifacts":{"old":{"path":"","basis":true}},"components":[{"artifacts":["old"]}]}),
    ] {
        let snapshot: Definitions = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(snapshot).unwrap(), wire);
    }
    let missing: crate::store::Run = serde_json::from_value(json!({
        "id":"run-old",
        "repoPath":"/repo",
        "stateDir":"/state",
        "status":"RUNNING",
        "createdAt":"2026-01-01T00:00:00Z",
        "selection":{"kind":"all"},
        "validation":null,
    }))
    .unwrap();
    assert!(missing.definitions.graph().is_none());
}

#[test]
fn current_graph_capture_serializes_identically_to_pinned_query_view() {
    let root = crate::test_os::tempdir();
    crate::test_declaration::write(
        root.path().join("index.artf"),
        json!({
            "name":"app",
            "views":{"agent_tools":{"read":{"builtin":"read"}}},
            "fingerprint":{"script":{"command":"echo","args":["v1"]}},
            "evals":[
                {
                    "id":"check",
                    "title":"Check",
                    "profile":{"kind":"agent","backend":"openai","model":"fixture"},
                    "payload":{"instruction":"Check.","owner":{"nested":[1,2]}},
                    "pass_schema":{"type":"object"},
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
    let config = config::read_workspace_config(root.path()).unwrap();
    let selection = Selection::All;
    let view = crate::query::graph(&config, &selection).unwrap();
    let before = serde_json::to_value(&view).unwrap();
    assert_eq!(
        before
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "artifacts",
            "components",
            "evals",
            "relations",
            "repoPath",
            "selection",
            "version"
        ]
    );
    assert_eq!(
        before["artifacts"]["app"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "basis",
            "children",
            "fingerprint",
            "kind",
            "mounts",
            "name",
            "path",
            "reviewPolicy",
            "tags",
            "views"
        ]
    );
    let snapshot = Definitions::from_view(&view).unwrap();
    assert_eq!(serde_json::to_value(snapshot).unwrap(), before);
    // The eval definition and reuse key are calculated from live declarations, never this
    // display snapshot.
    let eval_hash = crate::cache::eval_definition_hash(&config.evals[0].declaration);
    let saved_profile = &before["evals"][0]["declaration"]["profile"];
    assert_eq!(
        saved_profile,
        &json!({
            "kind":"agent",
            "backend":"openai",
            "model":"fixture",
            "reasoning":null,
            "timeoutMs":null,
            "maxToolCalls":null,
            "maxTokens":null,
        })
    );
    assert_eq!(
        eval_hash,
        "bc7083ad4c1041de824b8080a6bd576c8cc39caedd1db4b63d073ae973e6790b"
    );
    let fingerprints = BTreeMap::from([("app".parse().unwrap(), "v1".parse().unwrap())]);
    assert_eq!(
        crate::cache::key(&eval_hash, &fingerprints).as_str(),
        "4056b8c4b08cc6c34167297a0c9fbb6ca8c0ca28f996c1470805b690c7329350"
    );
}

#[tokio::test]
async fn saved_definition_timeouts_reject_unrepresentable_writes() {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir_all(&repo).unwrap();
    let receipts = crate::store::Receipts::open(&state, &repo).await.unwrap();
    let mut run: crate::store::Run = serde_json::from_value(json!({
        "id":"run-definition",
        "repoPath":repo,
        "stateDir":state,
        "status":"RUNNING",
        "createdAt":"2026-01-01T00:00:00Z",
        "selection":{"kind":"all"},
        "validation":null,
        "definitions":{
            "artifacts":{
                "app":{"fingerprint":{"script":{"command":"echo","args":[],"timeoutMs":1}}},
            },
        },
    }))
    .unwrap();
    receipts.create_run(&run, &[]).await.unwrap();
    let wire = json!({
        "artifacts":{"app":{"fingerprint":{"script":{"command":"echo","args":[],"timeoutMs":1}}}},
    });
    for duration in [
        std::time::Duration::ZERO,
        std::time::Duration::from_millis(2147483648),
        std::time::Duration::from_nanos(1),
    ] {
        let mut snapshot: Definitions = serde_json::from_value(wire.clone()).unwrap();
        let artifact = snapshot
            .0
            .as_mut()
            .unwrap()
            .artifacts
            .value()
            .unwrap()
            .get("app")
            .unwrap()
            .clone();
        let mut artifact = artifact;
        let mut fingerprint = artifact.fingerprint.value().unwrap().clone();
        let mut script = fingerprint.script.value().unwrap().clone();
        script.timeout_ms = Field::Value(duration);
        fingerprint.script = Field::Value(script);
        artifact.fingerprint = Field::Value(fingerprint);
        snapshot.0.as_mut().unwrap().artifacts =
            Field::Value(BTreeMap::from([("app".parse().unwrap(), artifact)]));
        assert!(serde_json::to_value(&snapshot).is_err());
        run.definitions = snapshot;
        assert!(receipts.save_run(&run).await.is_err());
        let saved = crate::store::read_run(&state, "run-definition")
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(saved.run.definitions).unwrap(), wire);
        run.id = "run-invalid-definition".parse().unwrap();
        assert!(receipts.create_run(&run, &[]).await.is_err());
        assert!(
            crate::store::read_run(&state, "run-invalid-definition")
                .await
                .is_err()
        );
        run.id = "run-definition".parse().unwrap();
    }
}
