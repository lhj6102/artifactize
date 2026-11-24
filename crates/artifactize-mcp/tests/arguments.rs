use artifactize_mcp::{Launch, Server};
use rmcp::{RoleServer, model::*, service::serve_directly};
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
#[expect(
    deprecated,
    reason = "the fixture declares roots for the legacy protocol"
)]
async fn invalid_calls_are_rejected_without_asking_for_client_roots() {
    let (server_io, client_io) = io::duplex(4096);
    let peer = InitializeRequestParams::new(
        ClientCapabilities::builder().enable_roots().build(),
        Implementation::new("test-client", "1"),
    )
    .with_protocol_version(ProtocolVersion::V_2024_11_05);
    let server = serve_directly::<RoleServer, _, _, _, _>(
        Server::new(Launch {
            repo: None,
            state: None,
            executable: PathBuf::new(),
        }),
        server_io,
        Some(peer),
    );
    let (input, mut output) = io::split(client_io);
    let mut incoming = BufReader::new(input).lines();

    let invalid = [
        (
            "verify_start",
            json!({"scope":{"kind":"all"},"jobs":0}),
            "Invalid arguments:",
        ),
        (
            "verify_wait",
            json!({"run":"run-test","seconds":41}),
            "seconds must be an integer from 0 through 40",
        ),
        (
            "status",
            json!({"scope":{"kind":"artifacts","artifactIds":[]}}),
            "scope arrays must contain 1–64 IDs",
        ),
        (
            "config_graph",
            json!({"scope":{"kind":"evals","evalIds":vec!["app/check"; 65]}}),
            "scope arrays must contain 1–64 IDs",
        ),
        (
            "request_logs",
            json!({"id":"request-test","tail":2,"start":1}),
            "tail and range are exclusive",
        ),
        ("run_list", json!({"limit":21}), "limit must be at most 20"),
        (
            "config_check",
            json!({"cursor":17}),
            "cursor must be a string",
        ),
        (
            "config_check",
            json!({"cursor":format!("{}:0:0", "0".repeat(64))}),
            "Invalid detail cursor offset.",
        ),
        (
            "plugin_add",
            json!({"path":"relative/package"}),
            "plugin_add path must be absolute.",
        ),
        ("not_a_tool", json!({}), "Unknown tool:"),
    ];
    for (id, (name, arguments, expected)) in invalid.into_iter().enumerate() {
        let request = json!({"jsonrpc":"2.0", "id":id, "method":"tools/call", "params":{"name":name,"arguments":arguments}});
        output
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        // The first message must be the tool response, never the client's roots request.
        let line = incoming.next_line().await.unwrap().unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], id, "{name}: {response}");
        assert_eq!(response["result"]["isError"], true, "{name}: {response}");
        let message = response["result"]["structuredContent"]["message"]
            .as_str()
            .unwrap();
        assert!(message.contains(expected), "{name}: {message}");
    }
    server.cancel().await.unwrap();
}
