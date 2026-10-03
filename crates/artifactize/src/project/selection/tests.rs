use std::{fs, path::Path};

use serde_json::json;

use super::*;
use crate::{
    config::{parse_declaration, read_workspace_config},
    graph::Graph,
};

fn fixture() -> RepoConfig {
    read_workspace_config(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime"))
        .unwrap()
}

fn critic_ids(selection: &Selection, config: &RepoConfig) -> Vec<String> {
    selection
        .resolve(config)
        .unwrap()
        .critics
        .iter()
        .map(|critic| critic.id.clone())
        .collect()
}

#[test]
fn ordered_union_keeps_first_occurrence_and_rejects_unknown_or_empty_roots() {
    let config = fixture();
    let artifacts = Selection::Artifacts {
        artifact_ids: vec![
            "green".into(),
            "cycle-b".into(),
            "green".into(),
            "cycle-a".into(),
        ],
    };
    let critics = Selection::Critics {
        critic_ids: vec![
            "green/check".into(),
            "cycle-b/check".into(),
            "green/check".into(),
            "cycle-a/check".into(),
        ],
    };
    let expected = ["green/check", "cycle-b/check", "cycle-a/check"];
    assert_eq!(critic_ids(&artifacts, &config), expected);
    assert_eq!(critic_ids(&critics, &config), expected);
    assert_eq!(
        artifacts.resolve(&config).unwrap().roots,
        ["green", "cycle-b", "cycle-a"]
    );
    assert_eq!(
        critic_ids(&Selection::All, &config),
        config
            .critics
            .iter()
            .map(|critic| critic.id.clone())
            .collect::<Vec<_>>()
    );
    let selected = Selection::Critic {
        critic_id: "cycle-a/check".into(),
    }
    .resolve(&config)
    .unwrap();
    assert_eq!(selected.critics.len(), 1);
    assert_eq!(
        Graph::new(&config)
            .unwrap()
            .dependency_closure(&selected.roots)
            .unwrap(),
        ["cycle-a", "cycle-b"]
    );
    assert!(
        critic_ids(
            &Selection::Artifact {
                artifact_id: "input".into()
            },
            &config
        )
        .is_empty()
    );
    for selection in [
        Selection::Artifact {
            artifact_id: "missing".into(),
        },
        Selection::Critic {
            critic_id: "green".into(),
        },
        Selection::Artifacts {
            artifact_ids: vec!["green".into(), "".into()],
        },
        Selection::Critics {
            critic_ids: vec!["green/check".into(), "missing".into()],
        },
        Selection::Artifacts {
            artifact_ids: vec![],
        },
        Selection::Critics { critic_ids: vec![] },
    ] {
        assert!(selection.resolve(&config).is_err(), "{selection:?}");
    }
}

#[test]
fn json_and_line_files_select_the_same_ordered_critics() {
    let directory = tempfile::tempdir().unwrap();
    let json_path = directory.path().join("selection.json");
    let line_path = directory.path().join("selection.txt");
    fs::write(
        &json_path,
        "\u{feff}[\"green\",\"cycle-b\",\"green\",\"cycle-a\"]",
    )
    .unwrap();
    fs::write(
        &line_path,
        "\u{feff} green \r\n\r\n cycle-b\r\ngreen\ncycle-a \n",
    )
    .unwrap();
    let json = read_selection_file(&json_path).unwrap();
    let lines = read_selection_file(&line_path).unwrap();
    assert_eq!(json, ["green", "cycle-b", "cycle-a"]);
    assert_eq!(json, lines);
    let config = fixture();
    assert_eq!(
        critic_ids(&Selection::Artifacts { artifact_ids: json }, &config),
        critic_ids(
            &Selection::Artifacts {
                artifact_ids: lines
            },
            &config
        )
    );
}

#[test]
fn selection_files_validate_before_deduplication_and_never_fallback_from_json() {
    assert_eq!(
        parse_selection_file("[broken").unwrap_err(),
        "Selection file contains invalid JSON."
    );
    for input in [
        "",
        " \r\n ",
        "[]",
        "[null]",
        "[1]",
        "[{}]",
        "[\"\"]",
        "[\" x\"]",
        "[\"x \"]",
        "x,y",
        "x y",
        "a\tb",
        "a\0b",
        "a\u{7f}b",
        "[\"x\\ny\"]",
        "[\"\u{feff}x\"]",
    ] {
        assert_eq!(
            parse_selection_file(input).unwrap_err(),
            FILE_FORMAT_ERROR,
            "{input:?}"
        );
    }
    assert_eq!(
        parse_selection_file("x\n".repeat(MAX_IDS).as_str()).unwrap(),
        ["x"]
    );
    assert_eq!(
        parse_selection_file("x\n".repeat(MAX_IDS + 1).as_str()).unwrap_err(),
        FILE_FORMAT_ERROR
    );
    let ids = vec!["x"; MAX_IDS + 1];
    assert_eq!(
        parse_selection_file(&serde_json::to_string(&ids).unwrap()).unwrap_err(),
        FILE_FORMAT_ERROR
    );
    let text = "x".repeat(MAX_FILE_BYTES);
    assert_eq!(
        parse_selection_file(&text).unwrap()[0].len(),
        MAX_FILE_BYTES
    );
    assert_eq!(
        parse_selection_file(&(text + "x")).unwrap_err(),
        "Selection file exceeds 4 MiB."
    );
}

#[test]
fn file_reads_reject_nonregular_and_oversized_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ids");
    let file = fs::File::create(&path).unwrap();
    file.set_len(MAX_FILE_BYTES as u64 + 1).unwrap();
    let error = "Selection input must be a regular file no larger than 4 MiB.";
    assert_eq!(read_selection_file(&path).unwrap_err(), error);
    assert_eq!(read_selection_file(directory.path()).unwrap_err(), error);
    assert_eq!(
        read_selection_file(Path::new("/dev/null")).unwrap_err(),
        error
    );
    let fifo = directory.path().join("fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(read_selection_file(&fifo).unwrap_err(), error);
    assert!(read_selection_file(&directory.path().join("missing")).is_err());
}

fn profile_fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("artifactize.json"), json!({
        "name":"target", "critics":[
            {"id":"z", "title":"Z", "profile":{"kind":"runtime","command":"/bin/true","args":[]},
             "profileVariants":{"careful":{"kind":"runtime","command":"/bin/echo","args":["{input}"],"timeoutMs":9}},
             "payload":{"instruction":"Check."}},
            {"id":"a", "title":"A", "profile":{"kind":"runtime","command":"/bin/true","args":[]},
             "payload":{"instruction":"Check."}}
        ]
    }).to_string()).unwrap();
    fs::create_dir(directory.path().join("external")).unwrap();
    fs::write(
        directory.path().join("external/artifactize.json"),
        r#"{"name":"input","basis":true}"#,
    )
    .unwrap();
    directory
}

#[test]
fn named_profiles_only_apply_to_included_critics_and_rebuild_runtime_dependencies() {
    let directory = profile_fixture();
    let source = directory.path().join("artifactize.json");
    let before = fs::read(&source).unwrap();
    let load = || read_workspace_config(directory.path()).unwrap();
    let selection = Selection::Critic {
        critic_id: "target/z".into(),
    };
    let selected = select_profiles(
        load(),
        &selection,
        Some(&ProfileSelection::Named("careful".into())),
        false,
    )
    .unwrap();
    assert_eq!(selected.critics[0].deps, ["input"]);
    assert_eq!(
        serde_json::to_value(&selected.critics[0].declaration.profile).unwrap()["command"],
        "/bin/echo"
    );
    assert_eq!(
        serde_json::to_value(&selected.critics[0].declaration.profile).unwrap()["timeoutMs"],
        9
    );
    assert_eq!(
        serde_json::to_value(&selected.critics[1].declaration.profile).unwrap()["command"],
        "/bin/true"
    );
    assert_eq!(
        critic_ids(
            &Selection::Artifact {
                artifact_id: "target".into()
            },
            &selected
        ),
        ["target/z", "target/a"]
    );
    assert_eq!(fs::read(source).unwrap(), before);
    assert!(load().critics[0].deps.is_empty());
    assert!(
        select_profiles(
            load(),
            &Selection::All,
            Some(&ProfileSelection::Named("careful".into())),
            false,
        )
        .unwrap_err()
        .contains("Unknown profile variant for target/a")
    );
    assert!(
        select_profiles(
            load(),
            &selection,
            Some(&ProfileSelection::Named("missing".into())),
            false,
        )
        .unwrap_err()
        .contains("Unknown profile variant for target/z")
    );
    let mapping =
        ProfileSelection::Critics(BTreeMap::from([("target/z".into(), "careful".into())]));
    assert!(select_profiles(load(), &Selection::All, Some(&mapping), false).is_ok());
    let outside =
        ProfileSelection::Critics(BTreeMap::from([("target/a".into(), "careful".into())]));
    assert!(
        select_profiles(load(), &selection, Some(&outside), false)
            .unwrap_err()
            .contains("outside the submitted Critic scope: target/a")
    );
    assert!(serde_json::from_value::<ProfileSelection>(json!({"target/z":1})).is_err());
    assert!(serde_json::from_value::<ProfileSelection>(json!(["careful"])).is_err());
}

#[test]
fn variant_declarations_are_complete_bounded_and_keep_reviewer_kind() {
    let declaration = |variants| {
        json!({"name":"target", "critics":[{
        "id":"check", "title":"Check", "profile":{"kind":"runtime", "command":"/bin/true","args":[]},
        "payload":{"instruction":"Check."}, "profileVariants": variants
    }]}).to_string()
    };
    let valid = json!({"kind":"runtime", "command":"/bin/false", "args":[], "timeoutMs":1});
    let variants: BTreeMap<_, _> = (0..64).map(|i| (format!("v{i}"), valid.clone())).collect();
    assert!(parse_declaration(&declaration(json!(variants))).is_ok());
    let mut oversized = variants;
    oversized.insert("extra".into(), valid.clone());
    assert!(
        parse_declaration(&declaration(json!(oversized)))
            .unwrap_err()
            .contains("at most 64")
    );
    for variants in [
        json!({"bad name":valid}),
        json!({"v":{"kind":"human"}}),
        json!({"v":{"timeoutMs":1}}),
        json!({"v":{"kind":"runtime","command":"/bin/true","args":[],"timeoutMs":0}}),
        json!(null),
    ] {
        assert!(parse_declaration(&declaration(variants)).is_err());
    }
}
