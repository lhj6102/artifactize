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

fn eval_ids(selection: &Selection, config: &RepoConfig) -> Vec<String> {
    selection
        .resolve(config)
        .unwrap()
        .evals
        .iter()
        .map(|eval| eval.id.to_string())
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
    let evals = Selection::Evals {
        eval_ids: vec![
            "green/check".into(),
            "cycle-b/check".into(),
            "green/check".into(),
            "cycle-a/check".into(),
        ],
    };
    let expected = ["green/check", "cycle-b/check", "cycle-a/check"];
    assert_eq!(eval_ids(&artifacts, &config), expected);
    assert_eq!(eval_ids(&evals, &config), expected);
    assert_eq!(
        artifacts.resolve(&config).unwrap().roots,
        ["green", "cycle-b", "cycle-a"]
    );
    assert_eq!(
        eval_ids(&Selection::All, &config),
        config
            .evals
            .iter()
            .map(|eval| eval.id.clone())
            .collect::<Vec<_>>()
    );
    let selected = Selection::Eval {
        eval_id: "cycle-a/check".into(),
    }
    .resolve(&config)
    .unwrap();
    assert_eq!(selected.evals.len(), 1);
    assert_eq!(
        Graph::new(&config)
            .unwrap()
            .dependency_closure(&selected.roots)
            .unwrap(),
        ["cycle-a", "cycle-b"]
    );
    assert!(
        eval_ids(
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
        Selection::Eval {
            eval_id: "green".into(),
        },
        Selection::Artifacts {
            artifact_ids: vec!["green".into(), "".into()],
        },
        Selection::Evals {
            eval_ids: vec!["green/check".into(), "missing".into()],
        },
        Selection::Artifacts {
            artifact_ids: vec![],
        },
        Selection::Evals { eval_ids: vec![] },
    ] {
        assert!(selection.resolve(&config).is_err(), "{selection:?}");
    }
}

#[test]
fn json_and_line_files_select_the_same_ordered_evals() {
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
        eval_ids(&Selection::Artifacts { artifact_ids: json }, &config),
        eval_ids(
            &Selection::Artifacts {
                artifact_ids: lines
            },
            &config
        )
    );
}

#[test]
fn selection_files_validate_before_deduplication_and_never_fallback_from_json() {
    for malformed in [
        "[broken",
        "[1,",
        "[null,]",
        "[{}] trailing",
        "[null,1e400]",
        "[null,{\"nested\":[1e400]}]",
    ] {
        assert_eq!(
            parse_selection_file(malformed).unwrap_err(),
            "Selection file contains invalid JSON."
        );
    }
    assert_eq!(
        parse_selection_file("\u{feff}[\"x\",\"y\",\"x\"]\r\n").unwrap(),
        ["x", "y"]
    );
    assert_eq!(
        parse_selection_file("\u{feff} x\r\n y\r\n").unwrap(),
        ["x", "y"]
    );
    // Non-array-looking input remains the line-file form, not JSON fallback.
    assert_eq!(parse_selection_file("null\r\n17").unwrap(), ["null", "17"]);
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
        "[18446744073709551616]",
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
    // The null device is no regular file on any system.
    assert!(read_selection_file(crate::test_os::null_device()).is_err());
    // Only Unix has FIFOs.
    #[cfg(unix)]
    {
        let fifo = directory.path().join("fifo");
        crate::test_os::fifo(&fifo);
        assert_eq!(read_selection_file(&fifo).unwrap_err(), error);
    }
    assert!(read_selection_file(&directory.path().join("missing")).is_err());
}

fn profile_fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    crate::test_declaration::write(
        directory.path().join("index.artf"),
        json!({
            "name":"target",
            "evals":[
                {
                    "id":"z",
                    "title":"Z",
                    "profile":{"kind":"runtime","command":"/bin/true","args":[]},
                    "profile_variants":{
                        "careful":{
                            "kind":"runtime",
                            "command":"/bin/echo",
                            "args":["{input}"],
                            "timeout_ms":9,
                        },
                    },
                    "payload":{"instruction":"Check."},
                },
                {
                    "id":"a",
                    "title":"A",
                    "profile":{"kind":"runtime","command":"/bin/true","args":[]},
                    "payload":{"instruction":"Check."},
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
    fs::create_dir(directory.path().join("external")).unwrap();
    crate::test_declaration::write(
        directory.path().join("external/index.artf"),
        r#"{"name":"input","basis":true}"#,
    )
    .unwrap();
    directory
}

#[test]
fn named_profiles_only_apply_to_included_evals_and_rebuild_runtime_dependencies() {
    let directory = profile_fixture();
    let source = directory.path().join("index.artf");
    let before = fs::read(&source).unwrap();
    let load = || read_workspace_config(directory.path()).unwrap();
    let selection = Selection::Eval {
        eval_id: "target/z".into(),
    };
    let selected = select_profiles(
        load(),
        &selection,
        Some(&ProfileSelection::Named("careful".into())),
        false,
    )
    .unwrap();
    assert_eq!(selected.evals[1].deps, ["input"]);
    assert_eq!(
        serde_json::to_value(&selected.evals[1].declaration.profile).unwrap()["command"],
        "/bin/echo"
    );
    assert_eq!(
        serde_json::to_value(&selected.evals[1].declaration.profile).unwrap()["timeoutMs"],
        9
    );
    assert_eq!(
        serde_json::to_value(&selected.evals[0].declaration.profile).unwrap()["command"],
        "/bin/true"
    );
    assert_eq!(
        eval_ids(
            &Selection::Artifact {
                artifact_id: "target".into()
            },
            &selected
        ),
        ["target/a", "target/z"]
    );
    assert_eq!(fs::read(source).unwrap(), before);
    assert!(load().evals[0].deps.is_empty());
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
    let mapping = ProfileSelection::Evals(BTreeMap::from([("target/z".into(), "careful".into())]));
    assert!(select_profiles(load(), &Selection::All, Some(&mapping), false).is_ok());
    let outside = ProfileSelection::Evals(BTreeMap::from([("target/a".into(), "careful".into())]));
    assert!(
        select_profiles(load(), &selection, Some(&outside), false)
            .unwrap_err()
            .contains("outside the submitted Eval scope: target/a")
    );
    assert!(serde_json::from_value::<ProfileSelection>(json!({"target/z":1})).is_err());
    assert!(serde_json::from_value::<ProfileSelection>(json!(["careful"])).is_err());
}

#[test]
fn variant_declarations_are_complete_bounded_and_keep_reviewer_kind() {
    let declaration = |variants| {
        crate::test_declaration::to_toml(json!({
            "name":"target",
            "evals":[
                {
                    "id":"check",
                    "title":"Check",
                    "profile":{"kind":"runtime", "command":"/bin/true","args":[]},
                    "payload":{"instruction":"Check."},
                    "profile_variants": variants,
                },
            ],
        }))
        .unwrap()
    };
    let valid = json!({"kind":"runtime", "command":"/bin/false", "args":[], "timeout_ms":1});
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
        json!({"v":{"timeout_ms":1}}),
        json!({"v":{"kind":"runtime","command":"/bin/true","args":[],"timeout_ms":0}}),
    ] {
        assert!(parse_declaration(&declaration(variants)).is_err());
    }
}
