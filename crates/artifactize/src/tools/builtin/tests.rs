use std::{
    fs,
    os::unix::{fs::symlink, net::UnixListener},
    path::PathBuf,
};

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::{
    config::read_workspace_config,
    tools::{Content, Registry, ToolResult},
};

struct Fixture {
    directory: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repo");
        fs::create_dir(&root).unwrap();
        let fixture = Self { directory, root };
        fixture.artifact("a", "a", json!({}), "Review.");
        fixture
    }

    fn artifact(&self, path: &str, name: &str, mounts: Value, instruction: &str) {
        self.write(&format!("{path}/artifactize.json"), json!({
            "name":name,"mounts":mounts,"views":{"agentTools":{
                "read":{"builtin":"read"},"list":{"builtin":"list"},"glob":{"builtin":"glob"},"grep":{"builtin":"grep"},"view_image":{"builtin":"view_image"}
            }},"evals":[{"id":"review","title":"Review","profile":{"kind":"agent","backend":"openai","model":"test","reasoning":"high"},"payload":{"instruction":instruction}}]
        }).to_string());
    }

    fn write(&self, path: &str, data: impl AsRef<[u8]>) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
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
        assert!(!result.is_error, "{result:?}");
        assert!(serde_json::to_vec(&result).unwrap().len() < RESULT_BYTES + 100);
        let [Content::Json { data }] = result.content.as_slice() else {
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
        assert!(!result.is_error, "{result:?}");
        assert_eq!(
            result.content,
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
                .is_error,
            "{path}"
        );
    }
    assert!(
        fixture
            .call("view_image_hidden", json!({"path":"secret"}))
            .await
            .is_error
    );
    for bytes in image::tests::unsupported() {
        fixture.write("a/invalid.png", bytes);
        assert!(
            fixture
                .call("view_image_a", json!({"path":"invalid.png"}))
                .await
                .is_error
        );
    }
    let mut bytes = image::tests::fixtures()[1].1.clone();
    bytes.resize(image::IMAGE_LIMIT, 0);
    fixture.write("a/large", &bytes);
    assert!(
        !fixture
            .call("view_image_a", json!({"path":"large"}))
            .await
            .is_error
    );
    bytes.push(0);
    fixture.write("a/large", bytes);
    assert!(
        fixture
            .call("view_image_a", json!({"path":"large"}))
            .await
            .is_error
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
            .is_error
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
                .is_error
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
            .is_error
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
    symlink("data/file", fixture.root.join("a/link")).unwrap();
    symlink("data", fixture.root.join("a/linkdir")).unwrap();
    symlink(fixture.directory.path(), fixture.root.join("a/escape")).unwrap();
    let _socket = UnixListener::bind(fixture.root.join("a/socket")).unwrap();
    let fifo = std::ffi::CString::new(fixture.root.join("a/fifo").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    for path in [
        "../outside/secret",
        "/etc/passwd",
        "data/../artifactize.json",
        "data//file",
        "data/./file",
        "data\\file",
        "link",
        "linkdir/file",
        "escape",
        "socket",
        "fifo",
    ] {
        for tool in ["read_a", "list_a", "glob_a", "grep_a", "view_image_a"] {
            let mut args = json!({"path":path});
            if tool == "glob_a" || tool == "grep_a" {
                args["pattern"] = json!(".*");
            }
            assert!(fixture.call(tool, args).await.is_error, "{tool} {path}");
        }
    }
    assert!(
        fixture
            .call("read_a", json!({"path":"data"}))
            .await
            .is_error
    );
    let data = fixture.data("glob_a", json!({"pattern":"**/*"})).await;
    assert_eq!(data["files"], json!(["artifactize.json", "data/file"]));
    let config = read_workspace_config(&fixture.root).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    fs::rename(fixture.root.join("a"), fixture.root.join("old-a")).unwrap();
    symlink("old-a", fixture.root.join("a")).unwrap();
    assert!(
        registry
            .call(
                "read_a",
                json!({"path":"data/file"}),
                &fixture.root,
                CancellationToken::new()
            )
            .await
            .is_error
    );
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
    ] {
        let result = fixture.call(tool, args).await;
        assert!(result.is_error);
        assert!(
            matches!(&result.content[0], Content::Text { text } if text.contains("Tool arguments"))
        );
    }
    fixture.write("bare/artifactize.json", r#"{"name":"bare"}"#);
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
            .call("read_bare", json!({"path":"artifactize.json"}))
            .await
            .is_error
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(
        registry
            .call("list_a", json!({}), &fixture.root, cancelled)
            .await
            .is_error
    );
}

#[tokio::test]
async fn listing_pages_are_sorted_and_include_logical_mounts_and_family_catalogs() {
    let fixture = Fixture::new();
    fixture.artifact("a", "a", json!({"source":"b"}), "Review.");
    fixture.artifact("b", "b", json!({"back":"a"}), "Refers to {hidden}.");
    fixture.artifact("hidden", "hidden", json!({}), "Review.");
    fixture.write("b/input.txt", "mounted\n");
    fixture.write(
        "a/nested/cases/artifactize.json",
        json!({"name":"cases","family":{"instances":{"one":{},"two":{}}}}).to_string(),
    );
    fixture.write("a/nested/cases/input.txt", "family\n");
    let data = fixture.data("list_a", json!({})).await;
    assert_eq!(
        data["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["artifactize.json", "nested", "source"]
    );
    assert_eq!(data["entries"][2]["kind"], "mount");
    let data = fixture.data("list_a", json!({"path":"nested"})).await;
    assert_eq!(
        data["entries"][0],
        json!({"name":"cases","path":"nested/cases","kind":"family","instances":["one","two"]})
    );
    let data = fixture
        .data("list_a", json!({"path":"nested/cases","limit":1}))
        .await;
    assert_eq!(data["totalEntries"], 2);
    assert_eq!(data["entries"][0]["path"], "nested/cases/one");
    assert_eq!(data["nextOffset"], 1);
    let data = fixture
        .data("list_a", json!({"path":"nested/cases","offset":1}))
        .await;
    assert_eq!(data["entries"][0]["name"], "two");
    assert_eq!(data["nextOffset"], Value::Null);
    let data = fixture
        .data("read_a", json!({"path":"nested/cases/one/input.txt"}))
        .await;
    assert_eq!(data["resolvedArtifactId"], "one");
    assert!(
        fixture
            .call("read_a", json!({"path":"nested/cases/input.txt"}))
            .await
            .is_error
    );
    assert_eq!(
        fixture
            .data("read_a", json!({"path":"source/input.txt"}))
            .await["resolvedArtifactId"],
        "b"
    );
    let data = fixture.data("glob_a", json!({"pattern":"**/*.txt"})).await;
    assert_eq!(
        data["files"],
        json!([
            "nested/cases/one/input.txt",
            "nested/cases/two/input.txt",
            "source/input.txt"
        ])
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
    symlink("a.txt", fixture.root.join("a/search/link.txt")).unwrap();
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
            .is_error
    );
    assert!(
        fixture
            .call("glob_a", json!({"pattern":"../*"}))
            .await
            .is_error
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
