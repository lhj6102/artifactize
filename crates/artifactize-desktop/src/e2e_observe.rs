//! Read-only observations of the CLI workers a domain fixture started.
//!
//! The WebdriverIO service owns the app and its driver. This executable never launches,
//! signals or cleans up either; it reuses the Rust tests' portable birth-identity observer
//! so deleting a fixture waits for its own durable CLI workers, including the supervisor.

#[expect(
    dead_code,
    reason = "this fixture only uses the saved-identity observation"
)]
#[path = "../../artifactize-core/tests/support/os/observation.rs"]
mod observation;

use std::io::{self, Read};

#[derive(serde::Deserialize)]
struct Identity {
    pid: artifactize_core::process::ProcessId,
    #[serde(deserialize_with = "start_time")]
    start_time: u64,
}

fn start_time<'de, D: serde::Deserializer<'de>>(input: D) -> Result<u64, D::Error> {
    let value = <serde_json::Value as serde::Deserialize>::deserialize(input)?;
    match value {
        serde_json::Value::String(decimal) => decimal.parse().map_err(serde::de::Error::custom),
        serde_json::Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("start_time must be an unsigned integer")),
        _ => Err(serde::de::Error::custom(
            "start_time must be decimal text or an unsigned integer",
        )),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `args` panics on a non-Unicode argument; compare the raw one instead.
    if std::env::args_os().nth(1).as_deref() != Some("alive".as_ref()) {
        return Err("usage: artifactize-e2e-observe alive < identities.json".into());
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let owners: Vec<Identity> = serde_json::from_str(&input)?;
    let running: Vec<_> = owners
        .into_iter()
        .map(|owner| observation::same_process_running(owner.pid.get(), owner.start_time))
        .collect();
    println!("{}", serde_json::to_string(&running)?);
    Ok(())
}
