use std::{fs, path::PathBuf};

use serde_json::{Value, json};
use tempfile::TempDir;

use artifactize_tools::builtin::{READ_BYTES, RESULT_BYTES, SEARCH_FILE_BYTES};
use tokio_util::sync::CancellationToken;

use super::image;
use crate::{
    config::read_workspace_config,
    test_os::{symlink_dir, symlink_file},
    tools::{Content, Registry, ToolResult},
};

struct Fixture {
    directory: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = crate::test_os::tempdir();
        let root = directory.path().join("repo");
        fs::create_dir(&root).unwrap();
        let fixture = Self { directory, root };
        fixture.artifact("a", "a", json!({}), "Review.");
        fixture
    }

    fn artifact(&self, path: &str, name: &str, mounts: Value, instruction: &str) {
        self.write(
            &format!("{path}/index.artf"),
            json!({
                "name":name,
                "mounts":mounts,
                "views":{
                    "agent_tools":{
                        "read":{"builtin":"read"},
                        "list":{"builtin":"list"},
                        "glob":{"builtin":"glob"},
                        "grep":{"builtin":"grep"},
                        "view_image":{"builtin":"view_image"},
                    },
                },
                "evals":[
                    {
                        "id":"review",
                        "title":"Review",
                        "profile":{
                            "kind":"agent",
                            "backend":"openai",
                            "model":"test",
                            "reasoning":"high",
                        },
                        "payload":{"instruction":instruction},
                    },
                ],
            })
            .to_string(),
        );
    }

    fn write(&self, path: &str, data: impl AsRef<[u8]>) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        crate::test_declaration::write(path, data).unwrap();
    }

    async fn call(&self, tool: &str, args: Value) -> ToolResult {
        let config = read_workspace_config(&self.root).unwrap();
        let registry = Registry::new(&config, "a/review").unwrap();
        let output = self.root.join("never-created");
        let result = registry
            .call(tool, args, &output, CancellationToken::new())
            .await;
        assert!(!output.exists());
        result
    }

    async fn data(&self, tool: &str, args: Value) -> Value {
        let result = self.call(tool, args).await;
        assert!(!result.is_error(), "{result:?}");
        assert!(serde_json::to_vec(&result).unwrap().len() < RESULT_BYTES + 100);
        let [Content::Json { data }] = result.content() else {
            panic!("{result:?}")
        };
        data.clone()
    }
}

#[tokio::test]
async fn view_image_detects_bytes_and_obeys_size_format_and_scope_limits() {
    use base64::{Engine, engine::general_purpose::STANDARD};

    let fixture = Fixture::new();
    fixture.artifact("a", "a", json!({"source":"b"}), "Review.");
    fixture.artifact("b", "b", json!({}), "Refers to {hidden}.");
    fixture.artifact("hidden", "hidden", json!({}), "Review.");
    fixture.write("hidden/secret", &image::tests::fixtures()[0].1);
    for (mime, bytes) in image::tests::fixtures() {
        fixture.write("b/image.txt", &bytes);
        let result = fixture
            .call("view_image_a", json!({"path":"source/image.txt"}))
            .await;
        assert!(!result.is_error(), "{result:?}");
        assert_eq!(
            result.content(),
            vec![Content::Image {
                data: STANDARD.encode(&bytes),
                mime_type: mime.into()
            }]
        );
    }
    for path in [
        "../hidden/secret",
        "hidden/secret",
        "source/../hidden/secret",
        "source",
        "",
    ] {
        assert!(
            fixture
                .call("view_image_a", json!({"path":path}))
                .await
                .is_error(),
            "{path}"
        );
    }
    assert!(
        fixture
            .call("view_image_hidden", json!({"path":"secret"}))
            .await
            .is_error()
    );
    for bytes in image::tests::unsupported() {
        fixture.write("a/invalid.png", bytes);
        assert!(
            fixture
                .call("view_image_a", json!({"path":"invalid.png"}))
                .await
                .is_error()
        );
    }
    let mut bytes = image::tests::fixtures()[1].1.clone();
    bytes.resize(image::IMAGE_LIMIT, 0);
    fixture.write("a/large", &bytes);
    assert!(
        !fixture
            .call("view_image_a", json!({"path":"large"}))
            .await
            .is_error()
    );
    bytes.push(0);
    fixture.write("a/large", bytes);
    assert!(
        fixture
            .call("view_image_a", json!({"path":"large"}))
            .await
            .is_error()
    );
}

#[tokio::test]
async fn read_preserves_lines_and_reports_eof_and_partial_pages() {
    let fixture = Fixture::new();
    fixture.write("a/text", "\u{feff}one\r\n\n三\nlast");
    let data = fixture
        .data("read_a", json!({"path":"text","limit":2}))
        .await;
    assert_eq!(
        data["lines"],
        json!([{"number":1,"text":"\u{feff}one\r\n"},{"number":2,"text":"\n"}])
    );
    assert_eq!(data["startLine"], 1);
    assert_eq!(data["endLine"], 2);
    assert_eq!(data["lineCount"], 2);
    assert_eq!(data["truncated"], true);
    assert_eq!(data["nextOffset"], 3);
    assert!(data.get("totalLines").is_none());
    let data = fixture
        .data("read_a", json!({"path":"text","offset":3}))
        .await;
    assert_eq!(
        data["lines"],
        json!([{"number":3,"text":"三\n"},{"number":4,"text":"last"}])
    );
    assert_eq!(data["totalLines"], 4);
    assert_eq!(data["endLine"], 4);
    assert_eq!(data["nextOffset"], Value::Null);
    assert_eq!(data["truncated"], false);
    let data = fixture
        .data("read_a", json!({"path":"text","offset":20}))
        .await;
    assert_eq!(data["totalLines"], 4);
    assert_eq!(data["lineCount"], 0);
    assert_eq!(data["endLine"], Value::Null);
    fixture.write("a/text", "");
    let data = fixture.data("read_a", json!({"path":"text"})).await;
    assert_eq!(data["totalLines"], 0);
    assert_eq!(data["lines"], json!([]));
    fixture.write("a/text", "a\nb\n");
    let data = fixture
        .data("read_a", json!({"path":"text","limit":2}))
        .await;
    assert_eq!(data["totalLines"], 2);
    assert_eq!(data["truncated"], false);
    fixture.write("a/text", "x\n".repeat(501));
    let data = fixture.data("read_a", json!({"path":"text"})).await;
    assert_eq!(data["lineCount"], 80);
    assert_eq!(data["nextOffset"], 81);
    let data = fixture
        .data("read_a", json!({"path":"text","limit":500}))
        .await;
    assert_eq!(data["lineCount"], 500);
    assert_eq!(data["nextOffset"], 501);
    let data = fixture
        .data("read_a", json!({"path":"text","offset":2.0,"limit":1.0}))
        .await;
    assert_eq!(data["lineCount"], 1);
    assert_eq!(data["startLine"], 2);
}

#[tokio::test]
async fn read_bounds_complete_lines_and_decodes_only_requested_lines() {
    let fixture = Fixture::new();
    fixture.write("a/text", format!("{}\nnext", "a".repeat(READ_BYTES - 1)));
    let data = fixture.data("read_a", json!({"path":"text"})).await;
    assert_eq!(data["lineCount"], 1);
    assert_eq!(data["nextOffset"], 2);
    fixture.write("a/text", format!("ok\n{}\n", "界".repeat(30_000)));
    let data = fixture.data("read_a", json!({"path":"text"})).await;
    assert_eq!(data["lines"], json!([{"number":1,"text":"ok\n"}]));
    assert!(
        fixture
            .call("read_a", json!({"path":"text","offset":2}))
            .await
            .is_error()
    );
    fixture.write("a/text", format!("{}\né\n", "a".repeat(READ_BYTES + 1)));
    let data = fixture
        .data("read_a", json!({"path":"text","offset":2}))
        .await;
    assert_eq!(data["lines"][0]["text"], "é\n");
    for invalid in [
        b"\xff\nvalid\n".as_slice(),
        b"\0\nvalid\n",
        b"\xc3\nvalid\n",
    ] {
        fixture.write("a/text", invalid);
        assert!(
            fixture
                .call("read_a", json!({"path":"text"}))
                .await
                .is_error()
        );
        let data = fixture
            .data("read_a", json!({"path":"text","offset":2}))
            .await;
        assert_eq!(data["lines"][0]["text"], "valid\n");
    }
    fixture.write("a/text", b"valid\n\xff\0");
    assert!(
        !fixture
            .call("read_a", json!({"path":"text","limit":1}))
            .await
            .is_error()
    );
    fixture.write("a/text", format!("{}é\r\n", "a".repeat(8191)));
    let data = fixture.data("read_a", json!({"path":"text"})).await;
    assert!(
        data["lines"][0]["text"]
            .as_str()
            .unwrap()
            .ends_with("é\r\n")
    );
}

#[tokio::test]
async fn paths_reject_escapes_links_and_nonregular_targets_without_writes() {
    let fixture = Fixture::new();
    fixture.write("a/data/file", "visible");
    fixture.write("outside/secret", "secret");
    let mut paths = vec![
        "../outside/secret",
        "/etc/passwd",
        "C:/Windows/win.ini",
        "data/../index.artf",
        "data//file",
        "data/./file",
        "data\\file",
    ];
    let a = fixture.root.join("a");
    if symlink_file(PathBuf::from("data").join("file"), a.join("link")).is_some() {
        paths.push("link");
    }
    if symlink_dir("data", a.join("linkdir")).is_some() {
        paths.push("linkdir/file");
    }
    if symlink_dir(fixture.directory.path(), a.join("escape")).is_some() {
        paths.push("escape");
    }
    // Sockets and FIFOs: entries that are neither files nor directories, which only Unix has.
    // Unix-only special-file or filesystem permission behavior.
    #[cfg(unix)]
    let _socket = crate::test_os::socket(&a.join("socket"));
    // Unix-only special-file or filesystem permission behavior.
    #[cfg(unix)]
    {
        crate::test_os::fifo(&a.join("fifo"));
        paths.extend(["socket", "fifo"]);
    }
    // Junctions redirect like directory symlinks and need no privilege.
    // Windows-only junction/reparse behavior or unprivileged link fallback.
    #[cfg(windows)]
    {
        crate::test_os::junction(&a.join("data"), &a.join("joined"));
        crate::test_os::junction(fixture.directory.path(), &a.join("escape-junction"));
        paths.extend(["joined", "joined/file", "escape-junction"]);
    }
    for path in paths {
        for tool in ["read_a", "list_a", "glob_a", "grep_a", "view_image_a"] {
            let mut args = json!({"path":path});
            if tool == "glob_a" || tool == "grep_a" {
                args["pattern"] = json!(".*");
            }
            assert!(fixture.call(tool, args).await.is_error(), "{tool} {path}");
        }
    }
    assert!(
        fixture
            .call("read_a", json!({"path":"data"}))
            .await
            .is_error()
    );
    let data = fixture.data("glob_a", json!({"pattern":"**/*"})).await;
    assert_eq!(data["files"], json!(["data/file", "index.artf"]));
    let config = read_workspace_config(&fixture.root).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    fs::rename(fixture.root.join("a"), fixture.root.join("old-a")).unwrap();
    let read = || {
        registry.call(
            "read_a",
            json!({"path":"data/file"}),
            &fixture.root,
            CancellationToken::new(),
        )
    };
    if symlink_dir("old-a", fixture.root.join("a")).is_some() {
        assert!(read().await.is_error());
    }
    // Windows-only junction/reparse behavior or unprivileged link fallback.
    #[cfg(windows)]
    {
        let _ = fs::remove_dir(fixture.root.join("a"));
        crate::test_os::junction(&fixture.root.join("old-a"), &fixture.root.join("a"));
        assert!(read().await.is_error());
    }
}

#[tokio::test]
async fn missing_paths_and_links_report_their_cause() {
    let fixture = Fixture::new();
    fixture.write("a/data/file", "visible");
    let error = |result: ToolResult| {
        assert!(result.is_error(), "{result:?}");
        let [Content::Text { text }] = result.content() else {
            panic!("{result:?}")
        };
        text.clone()
    };
    let symlink = "Cannot open Artifact input without symlink traversal.";
    let mut cases = vec![
        ("read_a", "notes.md", r#"No "notes.md" in Artifact a."#),
        (
            "grep_a",
            "data/missing",
            r#"No "data/missing" in Artifact a."#,
        ),
        (
            "read_a",
            "a/data/file",
            r#"No "a/data/file" in Artifact a; paths are relative to the Artifact (for example "data/file"), without its name."#,
        ),
        (
            "view_image_a",
            "a/image.png",
            r#"No "a/image.png" in Artifact a; paths are relative to the Artifact (for example "image.png"), without its name."#,
        ),
        (
            "list_a",
            "a",
            r#"No "a" in Artifact a; paths are relative to the Artifact, without its name (its root is "")."#,
        ),
        // Only the Artifact's own name, not a longer component that starts with it, earns the hint.
        ("list_a", "ab/data", r#"No "ab/data" in Artifact a."#),
    ];
    let a = fixture.root.join("a");
    if symlink_file(PathBuf::from("data").join("file"), a.join("link")).is_some() {
        cases.push(("read_a", "link", symlink));
    }
    if symlink_dir("data", a.join("linkdir")).is_some() {
        cases.push(("list_a", "linkdir", symlink));
    }
    // Windows-only junction/reparse behavior or unprivileged link fallback.
    #[cfg(windows)]
    {
        crate::test_os::junction(&a.join("data"), &a.join("joined"));
        cases.extend([
            ("list_a", "joined", symlink),
            ("read_a", "joined/file", symlink),
        ]);
    }
    for (tool, path, message) in cases {
        let mut args = json!({"path":path});
        if tool == "grep_a" {
            args["pattern"] = json!(".*");
        }
        assert_eq!(
            error(fixture.call(tool, args).await),
            message,
            "{tool} {path}"
        );
    }
    // Any other failure names the operating system's error; Unix's is ENOTDIR here.
    // Unix-only special-file or filesystem permission behavior.
    #[cfg(unix)]
    assert_eq!(
        error(fixture.call("read_a", json!({"path":"data/file/x"})).await),
        format!(
            "Cannot open Artifact input: {}",
            crate::test_os::not_a_directory()
        )
    );
}

#[tokio::test]
async fn descriptions_say_paths_are_relative_to_the_artifact_without_its_name() {
    let fixture = Fixture::new();
    let config = read_workspace_config(&fixture.root).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    for tool in registry.list() {
        let example = if tool.name == "view_image_a" {
            r#"use "image.png", not "a/image.png"."#
        } else {
            r#"use "notes.md", not "a/notes.md"."#
        };
        assert!(
            tool.description
                .contains(&format!("Paths are relative to a itself: {example}")),
            "{}",
            tool.description
        );
        assert!(
            tool.input_schema["properties"]["path"]["description"]
                .as_str()
                .unwrap()
                .starts_with(
                    "Logical path relative to the tool's Artifact, without the Artifact's name;"
                )
        );
    }
}

#[tokio::test]
async fn schemas_reject_unknown_or_out_of_range_arguments_and_listing_is_opt_in() {
    let fixture = Fixture::new();
    for (tool, args) in [
        ("read_a", json!({})),
        ("view_image_a", json!({})),
        ("view_image_a", json!({"path":"x","extra":true})),
        ("read_a", json!({"path":"x","offset":0})),
        ("read_a", json!({"path":"x","limit":501})),
        ("read_a", json!({"path":"x","extra":true})),
        ("list_a", json!({"offset":-1})),
        ("list_a", json!({"limit":201})),
        ("glob_a", json!({"pattern":""})),
        ("grep_a", json!({"pattern":"a","maxResults":201})),
        ("grep_a", json!({"pattern":"a","caseInsensitive":1})),
        ("read_a", json!({"path":null})),
        ("read_a", json!({"path":"x","offset":null})),
        ("read_a", json!({"path":"x","limit":null})),
        ("read_a", json!({"path":"x","offset":1.5})),
        ("read_a", json!({"path":"x","offset":9007199254740992_u64})),
        ("list_a", json!({"path":null})),
        ("list_a", json!({"offset":null})),
        ("list_a", json!({"limit":0})),
        ("list_a", json!({"limit":null})),
        ("glob_a", json!({})),
        ("glob_a", json!({"pattern":null})),
        ("glob_a", json!({"pattern":"*","extra":true})),
        ("grep_a", json!({})),
        ("grep_a", json!({"pattern":null})),
        ("grep_a", json!({"pattern":"a","glob":null})),
        ("grep_a", json!({"pattern":"a","maxResults":null})),
        ("grep_a", json!({"pattern":"a","caseInsensitive":null})),
        ("grep_a", json!({"pattern":"a","maxResults":0})),
        ("view_image_a", json!({"path":null})),
    ] {
        let result = fixture.call(tool, args).await;
        assert!(result.is_error());
        assert!(
            matches!(&result.content()[0], Content::Text { text } if text.contains("Tool arguments"))
        );
    }
    fixture.write("bare/index.artf", r#"{"name":"bare"}"#);
    fixture.artifact("other", "other", json!({}), "Review.");
    fixture.artifact("a", "a", json!({"bare":"bare"}), "Review.");
    let config = read_workspace_config(&fixture.root).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    assert_eq!(
        registry
            .list()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["glob_a", "grep_a", "list_a", "read_a", "view_image_a"]
    );
    for tool in registry.list() {
        assert!(!tool.description.contains("not yet implemented"));
        assert!(tool.input_schema["additionalProperties"] == false);
    }
    assert!(
        fixture
            .call("read_bare", json!({"path":"index.artf"}))
            .await
            .is_error()
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(
        registry
            .call("list_a", json!({}), &fixture.root, cancelled)
            .await
            .is_error()
    );
}

#[tokio::test]
async fn listing_pages_are_sorted_and_include_logical_mounts_and_children() {
    let fixture = Fixture::new();
    fixture.artifact("a", "a", json!({"source":"b"}), "Review.");
    fixture.artifact("b", "b", json!({"back":"a"}), "Refers to {hidden}.");
    fixture.artifact("hidden", "hidden", json!({}), "Review.");
    fixture.write("b/input.txt", "mounted\n");
    fixture.write(
        "a/nested/cases/index.artf",
        json!({"name":"cases"}).to_string(),
    );
    fixture.write("a/nested/cases/input.txt", "child\n");
    let data = fixture.data("list_a", json!({})).await;
    assert_eq!(
        data["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["index.artf", "nested", "source"]
    );
    assert_eq!(data["entries"][2]["kind"], "mount");
    let data = fixture.data("list_a", json!({"path":"nested"})).await;
    assert_eq!(
        data["entries"][0],
        json!({"name":"cases","path":"nested/cases","kind":"directory"})
    );
    let data = fixture
        .data("list_a", json!({"path":"nested/cases","limit":1}))
        .await;
    assert_eq!(data["totalEntries"], 2);
    assert_eq!(data["entries"][0]["path"], "nested/cases/index.artf");
    assert_eq!(data["nextOffset"], 1);
    let data = fixture
        .data("list_a", json!({"path":"nested/cases","offset":1}))
        .await;
    assert_eq!(data["entries"][0]["name"], "input.txt");
    assert_eq!(data["nextOffset"], Value::Null);
    let data = fixture
        .data("read_a", json!({"path":"nested/cases/input.txt"}))
        .await;
    assert_eq!(data["resolvedArtifactId"], "cases");
    assert_eq!(
        fixture
            .data("read_a", json!({"path":"source/input.txt"}))
            .await["resolvedArtifactId"],
        "b"
    );
    let data = fixture.data("glob_a", json!({"pattern":"**/*.txt"})).await;
    assert_eq!(
        data["files"],
        json!(["nested/cases/input.txt", "source/input.txt"])
    );
    assert_eq!(data["truncated"], false);
    let config = read_workspace_config(&fixture.root).unwrap();
    assert!(
        !Registry::new(&config, "a/review")
            .unwrap()
            .list()
            .any(|tool| tool.name.ends_with("_hidden"))
    );
    for i in 0..201 {
        fixture.write(&format!("a/page/{i:03}"), "");
    }
    let data = fixture.data("list_a", json!({"path":"page"})).await;
    assert_eq!(data["entries"].as_array().unwrap().len(), 200);
    assert_eq!(data["nextOffset"], 200);
    let data = fixture
        .data("list_a", json!({"path":"page","offset":200}))
        .await;
    assert_eq!(data["entries"][0]["name"], "200");
    assert_eq!(data["nextOffset"], Value::Null);
    let data = fixture
        .data("list_a", json!({"path":"page","offset":500}))
        .await;
    assert_eq!(data["entries"], json!([]));
    assert_eq!(data["totalEntries"], 201);
}

#[tokio::test]
async fn glob_and_grep_are_bounded_sorted_and_skip_binary_and_symlinks() {
    let fixture = Fixture::new();
    fixture.write("a/search/a.txt", "Alpha\r\nbeta\nALPHA beta\n");
    fixture.write("a/search/sub/b.txt", "alpha\n");
    fixture.write("a/search/.hidden.txt", "alpha\n");
    fixture.write("a/search/binary.txt", b"alpha\n\0");
    fixture.write("a/search/invalid.txt", b"alpha\n\xff");
    symlink_file("a.txt", fixture.root.join("a/search/link.txt"));
    // Windows-only junction/reparse behavior or unprivileged link fallback.
    #[cfg(windows)]
    crate::test_os::junction(
        &fixture.root.join("a/search/sub"),
        &fixture.root.join("a/search/joined.txt"),
    );
    let data = fixture
        .data("glob_a", json!({"path":"search","pattern":"*.txt"}))
        .await;
    assert_eq!(
        data["files"],
        json!([
            "search/.hidden.txt",
            "search/a.txt",
            "search/binary.txt",
            "search/invalid.txt"
        ])
    );
    let data = fixture
        .data(
            "grep_a",
            json!({"path":"search","pattern":"^alpha","glob":"**/*.txt","caseInsensitive":true}),
        )
        .await;
    assert_eq!(
        data["matches"],
        json!([
            {"path":"search/.hidden.txt","line":1,"text":"alpha"},
            {"path":"search/a.txt","line":1,"text":"Alpha"},
            {"path":"search/a.txt","line":3,"text":"ALPHA beta"},
            {"path":"search/sub/b.txt","line":1,"text":"alpha"}
        ])
    );
    assert_eq!(data["truncated"], false);
    let data = fixture
        .data(
            "grep_a",
            json!({"path":"search","pattern":"alpha","caseInsensitive":true,"maxResults":2}),
        )
        .await;
    assert_eq!(data["matches"].as_array().unwrap().len(), 2);
    assert_eq!(data["truncated"], true);
    let data = fixture
        .data(
            "grep_a",
            json!({"path":"search/a.txt","pattern":"beta$","maxResults":2}),
        )
        .await;
    assert_eq!(data["matches"].as_array().unwrap().len(), 2);
    assert_eq!(data["truncated"], false);
    assert!(
        fixture
            .call("grep_a", json!({"pattern":"["}))
            .await
            .is_error()
    );
    assert!(
        fixture
            .call("glob_a", json!({"pattern":"../*"}))
            .await
            .is_error()
    );
    for i in 0..201 {
        fixture.write(&format!("a/many/{i:03}.txt"), "match\n");
    }
    let data = fixture
        .data("glob_a", json!({"path":"many","pattern":"**/*.txt"}))
        .await;
    assert_eq!(data["files"].as_array().unwrap().len(), 200);
    assert_eq!(data["files"][0], "many/000.txt");
    assert_eq!(data["truncated"], true);
    let data = fixture
        .data("grep_a", json!({"path":"many","pattern":"match"}))
        .await;
    assert_eq!(data["matches"].as_array().unwrap().len(), 200);
    assert_eq!(data["truncated"], true);
    fixture.write("a/large", "x".repeat(SEARCH_FILE_BYTES + 1));
    let data = fixture
        .data("grep_a", json!({"path":"large","pattern":"x"}))
        .await;
    assert_eq!(data["matches"], json!([]));
    assert_eq!(data["truncated"], true);
    fixture.write("a/large", format!("ok\n{}\n", "x".repeat(RESULT_BYTES)));
    let data = fixture
        .data("grep_a", json!({"path":"large","pattern":"."}))
        .await;
    assert_eq!(data["matches"].as_array().unwrap().len(), 1);
    assert_eq!(data["truncated"], true);
}

#[tokio::test]
async fn typed_inputs_preserve_defaults_integral_float_pages_and_grep_options() {
    let fixture = Fixture::new();
    fixture.write("a/page.txt", "first\nSECOND\nlast\n");
    let read = fixture
        .data(
            "read_a",
            json!({"path":"page.txt","offset":2.0,"limit":1.0}),
        )
        .await;
    assert_eq!(read["lines"], json!([{"number":2,"text":"SECOND\n"}]));
    let read = fixture.data("read_a", json!({"path":"page.txt"})).await;
    assert_eq!(read["startLine"], 1);
    assert_eq!(read["lineCount"], 3);
    let list = fixture.data("list_a", json!({})).await;
    assert_eq!(list["path"], "");
    let grep = fixture
        .data(
            "grep_a",
            json!({"pattern":"second","glob":"*.txt","caseInsensitive":true,"maxResults":1.0}),
        )
        .await;
    assert_eq!(
        grep["matches"],
        json!([{"path":"page.txt","line":2,"text":"SECOND"}])
    );
    let grep = fixture.data("grep_a", json!({"pattern":"second"})).await;
    assert_eq!(grep["matches"], json!([]));
    let glob = fixture.data("glob_a", json!({"pattern":"*.txt"})).await;
    assert_eq!(glob["files"], json!(["page.txt"]));
}

#[tokio::test]
async fn file_artifact_image_view_is_limited_to_the_target() {
    let fixture = Fixture::new();
    let bytes = &image::tests::fixtures()[0].1;
    fixture.write("images/hero.png", bytes);
    fixture.write("images/sibling.png", bytes);
    fixture.write(
        "images/hero.png.artf",
        json!({"name":"hero","views":{"agent_tools":{"view_image":{"builtin":"view_image"}}}})
            .to_string(),
    );
    let config = read_workspace_config(&fixture.root).unwrap();
    let registry = Registry::for_artifact(&config, "hero").unwrap();
    let result = registry
        .call(
            "view_image_hero",
            json!({"path":"hero.png"}),
            &fixture.root.join("never-created"),
            CancellationToken::new(),
        )
        .await;
    assert!(!result.is_error(), "{result:?}");
    assert!(matches!(result.content(), [Content::Image { .. }]));
    let result = registry
        .call(
            "view_image_hero",
            json!({"path":"sibling.png"}),
            &fixture.root.join("never-created"),
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_error());
}
