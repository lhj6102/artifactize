use std::{fs, path::Path};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::config::{parse_declaration, read_workspace_config};
use crate::test_os::{bin, make_executable, symlink_dir};

struct Fixture {
    directory: TempDir,
    repo: PathBuf,
    output: PathBuf,
}

impl Fixture {
    fn new(tool: Value) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let output = directory.path().join("output");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&output).unwrap();
        // The paths tools see, which on Windows are long and without `\\?\`.
        let repo = crate::platform::canonicalize(&repo).unwrap();
        fs::write(output.join("retained"), "caller-owned").unwrap();
        write_artifact(&repo, "a", json!({"inspect":tool}), "Review a.");
        Self {
            directory,
            repo,
            output,
        }
    }

    fn script(&self, code: &str) {
        // Python writes pipes in the ANSI code page on Windows; the tools here write UTF-8.
        let code = if cfg!(windows) {
            format!("import sys\nsys.stdout.reconfigure(encoding='utf-8')\n{code}")
        } else {
            code.to_owned()
        };
        fs::write(self.repo.join("tool.py"), code).unwrap();
    }

    async fn call(&self, args: Value) -> ToolResult {
        let config = read_workspace_config(&self.repo).unwrap();
        let registry = Registry::new(&config, "a/review").unwrap();
        let result = registry
            .call("inspect_a", args, &self.output, CancellationToken::new())
            .await;
        assert_eq!(fs::read_dir(&self.output).unwrap().count(), 1);
        result
    }
}

fn write_artifact(path: &Path, name: &str, tools: Value, instruction: &str) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("artifactize.json"), json!({
        "name":name,
        "views":{"agentTools":tools},
        "evals":[{"id":"review","title":"Review","profile":{"kind":"agent","backend":"openai","model":"test","reasoning":"high"},"payload":{"instruction":instruction}}],
    }).to_string()).unwrap();
}

fn command() -> Value {
    json!({"description":"Inspect {artifactName}","protocol":"json","command":"python3","args":["tool.py"]})
}

fn text(result: &ToolResult) -> &str {
    let [Content::Text { text }] = result.content.as_slice() else {
        panic!("expected text: {result:?}")
    };
    text
}

#[test]
fn flat_declarations_validate_strictly_and_inertly() {
    let parse = |tool| {
        parse_declaration(&json!({"name":"a","views":{"agentTools":{"inspect":tool}}}).to_string())
    };
    assert!(parse(command()).is_ok());
    for builtin in ["read", "list", "glob", "grep", "view_image"] {
        assert!(parse(json!({"builtin":builtin})).is_ok());
    }
    for invalid in [
        json!({"builtin":"unknown"}),
        json!({"builtin":"read","command":"cat"}),
        json!({"builtin":"read","description":null}),
        json!({"builtin":"read","description":"   "}),
        json!({"builtin":"read","description":"Read {other}"}),
        json!({"metadata":{"description":"Old"},"script":{"command":"cat","args":[]}}),
    ] {
        assert!(parse(invalid.clone()).is_err(), "{invalid}");
    }
    for (key, value) in [
        ("description", json!("")),
        ("description", json!("{other}")),
        ("description", json!("x".repeat(4001))),
        ("inputSchema", json!({"type":"string"})),
        (
            "inputSchema",
            json!({"type":"object","properties":{"n":{"minimum":"no"}}}),
        ),
        (
            "inputSchema",
            json!({"type":"object","$ref":"https://example.invalid/schema"}),
        ),
        (
            "inputSchema",
            json!({"type":"object","$ref":"file:///etc/passwd"}),
        ),
        ("inputSchema", Value::Null),
        ("timeoutMs", json!(0)),
        ("timeoutMs", json!(2_147_483_648u64)),
        ("timeoutMs", json!(null)),
        ("executionPaths", json!(["../outside"])),
        ("executionPaths", json!(["same", "same"])),
        ("protocol", json!("shell")),
        ("observation", json!("content")),
        ("command", json!("{command}")),
    ] {
        let mut tool = command();
        tool[key] = value;
        assert!(parse(tool.clone()).is_err(), "{tool}");
    }
    let mut tool = command();
    tool["protocol"] = json!("plain");
    tool["args"] = json!(["--q={query}"]);
    assert!(parse(tool.clone()).is_err());
    tool["inputSchema"] =
        json!({"type":"object","properties":{"query":{"type":["string","null"]}}});
    assert!(parse(tool).is_ok());
}

#[tokio::test]
async fn argument_validation_precedes_spawn_and_output_creation() {
    let mut tool = command();
    tool["inputSchema"] = json!({
        "type":"object","$defs":{"positive":{"type":"integer","minimum":1}},
        "properties":{"n":{"$ref":"#/$defs/positive"}},"required":["n"],"additionalProperties":false
    });
    let fixture = Fixture::new(tool);
    fixture.script("open('spawned', 'w').write('yes')\nprint('{\"content\":[{\"type\":\"text\",\"text\":\"ran\"}]}')");
    let config = read_workspace_config(&fixture.repo).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    for args in [
        json!([]),
        json!({}),
        json!({"n":0}),
        json!({"n":"secret-input"}),
        json!({"n":1,"extra":"x".repeat(65536)}),
    ] {
        let result = registry
            .call(
                "inspect_a",
                args,
                &fixture.repo.join("must-not-create"),
                CancellationToken::new(),
            )
            .await;
        assert!(result.is_error);
        assert!(text(&result).contains("Tool arguments"));
        assert!(!text(&result).contains("secret-input"));
        assert!(text(&result).len() < 4096);
        assert!(!fixture.repo.join("spawned").exists());
        assert!(!fixture.repo.join("must-not-create").exists());
    }
    assert!(!fixture.call(json!({"n":1})).await.is_error);
    assert!(fixture.repo.join("spawned").exists());
}

#[tokio::test]
async fn json_context_has_private_paths_declared_material_and_owner_cwd() {
    let mut tool = command();
    tool["executionPaths"] = json!(["shared.txt"]);
    let fixture = Fixture::new(tool);
    fs::write(fixture.repo.join("shared.txt"), "material").unwrap();
    fixture.script(r#"import json, os, sys
request = json.load(sys.stdin)
context = request['context']
assert request['version'] == 1 and request['args'] == {}
assert type(request['version']) is int
assert os.getcwd() == context['artifactPath']
assert os.environ['HOME'] != os.environ['TMPDIR']
assert context['tmpDir'] == os.environ['TMPDIR']
assert context['outputDir'] == os.environ['ARTIFACTIZE_OUTPUT_DIR']
allowed = {'PATH','LANG','HOME','TMP','TEMP','TMPDIR','XDG_CACHE_HOME','ARTIFACTIZE_WORKSPACE_DIR','ARTIFACTIZE_OUTPUT_DIR','ARTIFACTIZE_TMP_DIR','LC_CTYPE'}
if os.name == 'nt':
    allowed |= {'USERPROFILE','APPDATA','LOCALAPPDATA','SYSTEMROOT','COMSPEC','PATHEXT'}
assert set(os.environ) <= allowed
print(json.dumps({'content':[{'type':'text','text':'ok'},{'type':'json','data':context}]}))
"#);
    let result = fixture.call(json!({})).await;
    assert!(!result.is_error, "{result:?}");
    let Content::Json { data } = &result.content[1] else {
        panic!()
    };
    assert_eq!(data["artifactId"], "a");
    assert_eq!(data["artifactPath"], json!(fixture.repo));
    assert_eq!(data["scope"]["a"]["path"], json!(fixture.repo));
    assert_eq!(data["scope"]["a"]["children"], json!({}));
    assert_eq!(
        data["executionPaths"]["shared.txt"],
        json!(fixture.repo.join("shared.txt"))
    );
    assert!(!Path::new(data["outputDir"].as_str().unwrap()).exists());
}

#[tokio::test]
async fn json_success_authored_error_and_credential_safe_failures() {
    let fixture = Fixture::new(command());
    fixture.script("print('{\"isError\":true,\"content\":[{\"type\":\"text\",\"text\":\"Choose a smaller range.\"}]}')");
    let result = fixture.call(json!({})).await;
    assert!(result.is_error);
    assert_eq!(text(&result), "Choose a smaller range.");
    fixture.script(
        "import sys\nprint('secret-output')\nprint('secret-stderr',file=sys.stderr)\nsys.exit(2)",
    );
    assert_eq!(
        text(&fixture.call(json!({})).await),
        "Agent tool execution failed."
    );
    fixture.script(
        "import os, signal\nos.kill(os.getpid(), getattr(signal, 'SIGKILL', signal.SIGTERM))",
    );
    assert_eq!(
        text(&fixture.call(json!({})).await),
        "Agent tool execution failed."
    );
    fixture.script("print('malformed secret-output')");
    assert_eq!(
        text(&fixture.call(json!({})).await),
        "Agent tool returned invalid output."
    );
    fixture.script("import sys\nprint('secret-stderr',file=sys.stderr)\nprint('{\"content\":[{\"type\":\"text\",\"text\":\"safe\"}]}')");
    assert_eq!(text(&fixture.call(json!({})).await), "safe");
}

#[test]
fn json_result_limits_and_strict_shapes() {
    let output = tempfile::tempdir().unwrap();
    let parse = |value: Value| result::parse(&serde_json::to_vec(&value).unwrap(), output.path());
    let block = json!({"type":"text","text":"ok"});
    for invalid in [
        json!({"content":[]}),
        json!({"content":vec![block.clone();33]}),
        json!({"content":[{"type":"text","text":"x".repeat(65537)}]}),
        json!({"content":[{"type":"json","data":"x".repeat(524287)}]}),
        json!({"content":[block.clone()],"observation":{"kind":"content"}}),
        json!({"content":[{"type":"image","data":""}]}),
        json!({"content":[{"type":"json"}]}),
        json!({"content":[{"type":"text","text":"ok","extra":1}]}),
        json!({"isError":true,"content":[block.clone(),block.clone()]}),
        json!({"isError":true,"content":[{"type":"json","data":{}}]}),
        json!({"isError":true,"content":[{"type":"text","text":" \n"}]}),
        json!({"isError":"true","content":[block.clone()]}),
    ] {
        assert!(parse(invalid.clone()).is_err(), "{invalid}");
    }
    assert!(parse(json!({"content":[{"type":"text","text":"x".repeat(65536)}]})).is_ok());
    assert!(parse(json!({"content":[{"type":"json","data":"x".repeat(524286)}]})).is_ok());
    assert!(parse(json!({"content":vec![block;32],"isError":false})).is_ok());
    assert!(
        result::parse(
            b"{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]} {}",
            output.path()
        )
        .is_err()
    );
}

#[tokio::test]
async fn json_capture_supports_large_blocks_but_rejects_truncation() {
    let fixture = Fixture::new(command());
    fixture
        .script("import json\nprint(json.dumps({'content':[{'type':'json','data':'x'*524286}]}))");
    assert!(!fixture.call(json!({})).await.is_error);
    fixture.script("print('x'*(16*1024*1024+1))");
    assert_eq!(
        text(&fixture.call(json!({})).await),
        "Agent tool returned invalid output."
    );
}

#[tokio::test]
async fn json_images_are_embedded_before_output_cleanup_and_invalid_files_are_tool_errors() {
    let fixture = Fixture::new(command());
    for (mime, bytes) in image::tests::fixtures() {
        let encoded = STANDARD.encode(&bytes);
        fixture.script(&format!(
            r#"import base64, json, os, sys
request = json.load(sys.stdin)
root = request['context']['outputDir']
os.mkdir(os.path.join(root, 'nested'))
path = os.path.join(root, 'nested', 'image')
open(path, 'wb').write(base64.b64decode('{encoded}'))
print(json.dumps({{'content':[
    {{'type':'image','path':path,'mimeType':'{mime}'}},
    {{'type':'image','path':'nested/image','mimeType':'{mime}'}},
    {{'type':'image','data':'{encoded}','mimeType':'{mime}'}}
]}}))
"#
        ));
        let result = fixture.call(json!({})).await;
        assert!(!result.is_error, "{result:?}");
        assert_eq!(
            result.content,
            vec![
                Content::Image {
                    data: encoded,
                    mime_type: mime.into()
                };
                3
            ]
        );
    }
    fs::write(fixture.repo.join("source"), &image::tests::fixtures()[0].1).unwrap();
    // Windows symlinks need a privilege; junctions, which redirect the same way, do not.
    let (file_link, directory_link) = if cfg!(windows) {
        (
            "path = 'link'; _winapi.CreateJunction(os.getcwd(), os.path.join(root, path))",
            "path = 'linkdir/source'; _winapi.CreateJunction(os.getcwd(), os.path.join(root, 'linkdir'))",
        )
    } else {
        (
            "path = 'link'; os.symlink(os.path.abspath('source'), os.path.join(root, path))",
            "path = 'linkdir/source'; os.symlink(os.getcwd(), os.path.join(root, 'linkdir'))",
        )
    };
    for setup in [
        "path = request['context']['artifactPath'] + '/source'",
        "path = '../tmp/image'; shutil.copy('source', os.path.join(root, path))",
        file_link,
        directory_link,
        "path = 'image'; open(os.path.join(root,path), 'wb').write(b'not a PNG')",
        "path = 'image'; open(os.path.join(root,path), 'wb').truncate(4*1024*1024+1)",
    ] {
        fixture.script(&format!("import json, os, shutil, sys\nif os.name == 'nt': import _winapi\nrequest = json.load(sys.stdin)\nroot = request['context']['outputDir']\n{setup}\nprint(json.dumps({{'content':[{{'type':'image','path':path,'mimeType':'image/png'}}]}}))"));
        let result = fixture.call(json!({})).await;
        assert!(result.is_error, "{setup}");
        assert_eq!(text(&result), "Agent tool returned invalid output.");
    }
}

#[tokio::test]
async fn plain_substitution_is_literal_single_pass_and_handles_escaping() {
    let mut tool = command();
    tool["protocol"] = json!("plain");
    tool["args"] = json!([
        "tool.py",
        "{query}",
        "--query={query}",
        "{n}",
        "{items}",
        "{{query}}",
        "{{\"literal\":true}}"
    ]);
    tool["inputSchema"] = json!({"type":"object","properties":{"query":{"type":"string"},"n":{"type":"integer"},"items":{"type":"array"}},"required":["query","n","items"]});
    let fixture = Fixture::new(tool);
    fixture
        .script("import json, sys\nassert sys.stdin.read() == ''\nprint(json.dumps(sys.argv[1:]))");
    let query = "a b ' \" ; $(touch injected)\n{n} \\ $HOME";
    let result = fixture
        .call(json!({"query":query,"n":42,"items":[1,true]}))
        .await;
    assert!(!result.is_error);
    let argv: Value = serde_json::from_str(text(&result)).unwrap();
    assert_eq!(
        argv,
        json!([
            query,
            format!("--query={query}"),
            "42",
            "[1,true]",
            "{query}",
            "{\"literal\":true}"
        ])
    );
    assert!(!fixture.repo.join("injected").exists());
    assert!(
        fixture
            .call(json!({"query":"\u{0}","n":42,"items":[]}))
            .await
            .is_error
    );
}

#[tokio::test]
async fn plain_output_is_clean_bounded_and_nonzero_is_error() {
    let mut tool = command();
    tool["protocol"] = json!("plain");
    let fixture = Fixture::new(tool);
    fixture.script("import sys\nsys.stdout.buffer.write(b'\\x1b[31mhello\\x1b[0m\\x00\\t\\n')\nprint('stderr-secret',file=sys.stderr)");
    assert_eq!(text(&fixture.call(json!({})).await), "hello\t\n");
    fixture.script(
        "import sys\nprint('domain detail')\nprint('stderr-secret',file=sys.stderr)\nsys.exit(3)",
    );
    let result = fixture.call(json!({})).await;
    assert!(result.is_error);
    assert!(text(&result).contains("domain detail"));
    assert!(!text(&result).contains("stderr-secret"));
    fixture.script("print('界'*100000)");
    let result = fixture.call(json!({})).await;
    assert!(!result.is_error);
    assert!(text(&result).len() <= 65536);
    assert!(text(&result).ends_with("[output truncated]"));
}

#[tokio::test]
async fn executables_use_path_or_owner_relative_paths_not_implicit_local_search() {
    let mut tool = command();
    tool["command"] = json!("unique-artifactize-tool-not-on-path");
    tool["args"] = json!([]);
    tool["protocol"] = json!("plain");
    let fixture = Fixture::new(tool.clone());
    let script = fixture.repo.join("unique-artifactize-tool-not-on-path");
    fs::write(&script, "#!/bin/sh\nprintf 'owner executable'\n").unwrap();
    make_executable(&script);
    assert!(fixture.call(json!({})).await.is_error);
    tool["command"] = json!("./unique-artifactize-tool-not-on-path");
    write_artifact(
        &fixture.repo,
        "a",
        json!({"inspect":tool.clone()}),
        "Review.",
    );
    assert_eq!(text(&fixture.call(json!({})).await), "owner executable");
    // Windows has no printf on every PATH; python3 is a test prerequisite anyway.
    if cfg!(windows) {
        tool["command"] = json!("python3");
        tool["args"] = json!(["-c", "import sys; sys.stdout.write('from PATH')"]);
    } else {
        tool["command"] = json!("printf");
        tool["args"] = json!(["%s", "from PATH"]);
    }
    write_artifact(
        &fixture.repo,
        "a",
        json!({"inspect":tool.clone()}),
        "Review.",
    );
    assert_eq!(text(&fixture.call(json!({})).await), "from PATH");
    tool["command"] = json!(bin("/usr/bin/printf"));
    tool["args"] = json!(["%s", "from PATH"]);
    write_artifact(&fixture.repo, "a", json!({"inspect":tool}), "Review.");
    assert_eq!(text(&fixture.call(json!({})).await), "from PATH");
}

#[tokio::test]
async fn path_preparation_rejects_scope_and_workspace_escapes() {
    let fixture = Fixture::new(command());
    fixture.script("open('spawned','w').write('yes')\n");
    if symlink_dir(fixture.directory.path(), fixture.repo.join("link")).is_none() {
        #[cfg(windows)]
        crate::test_os::junction(fixture.directory.path(), &fixture.repo.join("link"));
    }
    for (key, value) in [
        ("command", json!("../outside")),
        ("command", json!("link/outside")),
        ("executionPaths", json!(["link"])),
        ("executionPaths", json!(["missing"])),
        ("args", json!(["tool.py", "{outside}"])),
        ("args", json!(["tool.py", "{a}/../escape"])),
        ("args", json!(["tool.py", "{a}/link"])),
    ] {
        let mut tool = command();
        tool[key] = value;
        write_artifact(&fixture.repo, "a", json!({"inspect":tool}), "Review.");
        assert!(fixture.call(json!({})).await.is_error);
        assert!(!fixture.repo.join("spawned").exists());
    }
    write_artifact(&fixture.repo, "a", json!({"inspect":command()}), "Review.");
    let config = read_workspace_config(&fixture.repo).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    let result = registry
        .call(
            "inspect_a",
            json!({}),
            &fixture.repo.join("output"),
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_error);
    assert!(!fixture.repo.join("output").exists());
}

#[tokio::test]
async fn registry_admits_only_eval_scope_and_agent_audience() {
    let fixture = Fixture::new(command());
    write_artifact(&fixture.repo, "root", json!({}), "Review root.");
    write_artifact(
        &fixture.repo.join("a"),
        "a",
        json!({"inspect":command()}),
        "Inspect {b}.",
    );
    write_artifact(
        &fixture.repo.join("b"),
        "b",
        json!({"read":{"builtin":"read"}}),
        "Depends on {c}.",
    );
    write_artifact(
        &fixture.repo.join("b/child"),
        "child",
        json!({"list":{"builtin":"list"}}),
        "Review child.",
    );
    write_artifact(
        &fixture.repo.join("c"),
        "c",
        json!({"grep":{"builtin":"grep"}}),
        "Review c.",
    );
    let config = read_workspace_config(&fixture.repo).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    assert_eq!(
        registry
            .list()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["inspect_a", "list_child", "read_b"]
    );
    assert_eq!(registry.list().next().unwrap().description, "Inspect a");
    assert!(
        !registry
            .call(
                "read_b",
                json!({"path":"artifactize.json"}),
                &fixture.output,
                CancellationToken::new()
            )
            .await
            .is_error
    );
    assert!(
        text(
            &registry
                .call(
                    "grep_c",
                    json!({}),
                    &fixture.output,
                    CancellationToken::new()
                )
                .await
        )
        .contains("Unknown")
    );
    assert!(Registry::new(&config, "a/missing").is_err());
}

#[test]
fn concatenated_tool_name_collisions_are_rejected() {
    let fixture = Fixture::new(command());
    write_artifact(&fixture.repo, "root", json!({}), "Review.");
    write_artifact(
        &fixture.repo.join("a"),
        "a",
        json!({"inspect_b":{"builtin":"read"}}),
        "Review.",
    );
    write_artifact(
        &fixture.repo.join("b"),
        "b_a",
        json!({"inspect":{"builtin":"read"}}),
        "Review.",
    );
    let config = read_workspace_config(&fixture.repo).unwrap();
    let error = Registry::new(&config, "root/review").err().unwrap();
    assert!(error.contains("collide: inspect_b_a"));
}

#[tokio::test]
async fn timeouts_and_cancelled_calls_cleanup_owned_directories() {
    let mut tool = command();
    tool["timeoutMs"] = json!(50);
    let fixture = Fixture::new(tool);
    fixture.script("import time\ntime.sleep(60)");
    assert_eq!(
        text(&fixture.call(json!({})).await),
        "Agent tool timed out."
    );
    let config = read_workspace_config(&fixture.repo).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(
        text(
            &registry
                .call("inspect_a", json!({}), &fixture.output, cancel)
                .await
        )
        .contains("cancelled")
    );
    assert_eq!(fs::read_dir(&fixture.output).unwrap().count(), 1);
}

#[tokio::test]
async fn json_scope_contains_only_paths_mounts_and_children() {
    let fixture = Fixture::new(command());
    let mut root: Value =
        serde_json::from_str(&fs::read_to_string(fixture.repo.join("artifactize.json")).unwrap())
            .unwrap();
    root["mounts"] = json!({"alias":"leaf"});
    root["views"]["humanTools"] = json!({"humanOnly":{"description":"Human only","kind":"output","command":"missing","args":[]}});
    fs::write(fixture.repo.join("artifactize.json"), root.to_string()).unwrap();
    fs::create_dir_all(fixture.repo.join("cases")).unwrap();
    fs::write(
        fixture.repo.join("cases/artifactize.json"),
        json!({"name":"leaf"}).to_string(),
    )
    .unwrap();
    fs::write(fixture.repo.join("cases/input.txt"), "material").unwrap();
    fixture.script("import json,sys\nrequest=json.load(sys.stdin)\nprint(json.dumps({'content':[{'type':'json','data':request['context']['scope']}]}))");
    let config = read_workspace_config(&fixture.repo).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    assert_eq!(
        registry
            .list()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["inspect_a"]
    );
    let result = registry
        .call(
            "inspect_a",
            json!({}),
            &fixture.output,
            CancellationToken::new(),
        )
        .await;
    let [Content::Json { data }] = result.content.as_slice() else {
        panic!("{result:?}")
    };
    assert_eq!(data["a"]["mounts"], json!({"alias":"leaf"}));
    assert_eq!(data["a"]["children"], json!({"cases":"leaf"}));
    assert_eq!(data["leaf"]["path"], json!(fixture.repo.join("cases")));
    assert_eq!(
        data["leaf"],
        json!({"path":fixture.repo.join("cases"),"children":{},"mounts":{}})
    );
    assert_eq!(data["a"].as_object().unwrap().len(), 3);
}

#[tokio::test]
async fn dropping_call_cleans_process_before_removing_directories() {
    let fixture = Fixture::new(command());
    fixture.script("import os,time\nopen('started','w').write(str(os.getpid()))\ntime.sleep(60)");
    let config = read_workspace_config(&fixture.repo).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    let mut call = Box::pin(registry.call(
        "inspect_a",
        json!({}),
        &fixture.output,
        CancellationToken::new(),
    ));
    tokio::select! {
        result = &mut call => panic!("unexpected completion: {result:?}"),
        // The file exists before its PID is written; wait for the PID itself.
        result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while fs::read_to_string(fixture.repo.join("started"))
                .map_or(true, |pid| pid.parse::<u32>().is_err())
            {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }) => { result.unwrap(); },
    }
    let pid: u32 = fs::read_to_string(fixture.repo.join("started"))
        .unwrap()
        .parse()
        .unwrap();
    drop(call);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while fs::read_dir(&fixture.output).unwrap().count() != 1 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    #[cfg(unix)]
    assert!(!Path::new(&format!("/proc/{pid}")).exists());
    #[cfg(windows)]
    assert!(!crate::test_os::running(pid));
}

#[test]
fn schema_diagnostics_are_bounded_without_limiting_valid_arrays() {
    let validator = schema::compile(&json!({"type":"object","properties":{"items":{"type":"array","items":{"type":"integer"}}}})).unwrap();
    assert!(schema::validate(&validator, &json!({"items":vec![1;4096]})).is_ok());
    let key = "secret\n\t/".repeat(4000);
    let validator = schema::compile(&json!({"type":"object","properties":{&key:{"type":"integer"}},"additionalProperties":false})).unwrap();
    let args = json!({&key:"secret-value","a":1,"b":1,"c":1,"d":1,"e":1});
    let message = schema::validate(&validator, &args).unwrap_err();
    assert!(message.len() < 4096);
    assert!(!message.contains("secret-value"));
    assert!(!message.contains("secret\n\t"));
    assert!(message.lines().count() <= 6);
}
