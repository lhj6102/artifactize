//! A portable Python harness with file handshakes, not startup sleeps. Logs live
//! outside the session so tests can inspect cleanup after artifactize removes it.

use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const HARNESS: &str = r#"import json, pathlib, sys, time
PATH_TEXT_FUNCTION
mode, log = sys.argv[1], pathlib.Path(sys.argv[2])
def wait(path):
    while not path.exists() and log.exists(): time.sleep(0.01)
def record(kind, data):
    pending = log / ('.pending-' + kind + '-' + session.name)
    pending.write_text(json.dumps(data))
    pending.replace(log / (kind + '-' + session.name))
if mode == 'child':
    held = hold_session_file(session_directory() / ('held-' + sys.argv[3]))
    (log / ('child-' + sys.argv[3])).write_text(str(session_pid()))
    wait(log / 'release')
    exit_session(0)
request = json.load(sys.stdin) if mode not in ['plain'] else None
session = session_directory()
if mode in ['start', 'unready', 'exit', 'stop', 'hang-stop', 'fail-stop', 'readonly-stop']:
    assert request['version'] == 1 and request['args'] == {}
    assert isinstance(request['context']['scope'], dict)
if mode in ['start', 'unready', 'exit']:
    child = launch_session_child(log, session.name)
    wait(log / ('child-' + session.name))
    (session / 'evidence').write_text('started first')
    record('start', {'directory':path_text(session), 'pid':session_pid(), 'child':child.pid, 'home':session_home()})
    if mode == 'exit': exit_session(1)
    if mode == 'start': print('ready', flush=True)
    wait(session / 'exit')
elif mode in ['stop', 'hang-stop', 'fail-stop', 'readonly-stop']:
    child = None
    if mode == 'hang-stop':
        child = launch_session_child(log, 'stop-' + session.name)
        wait(log / ('child-stop-' + session.name))
    record('stop', {'directory':path_text(session), 'home':session_home(), 'pid':session_pid(), 'child':child.pid if child else None})
    if mode == 'hang-stop': wait(log / 'release')
    if mode == 'fail-stop': exit_session(1)
    if mode == 'readonly-stop': deny_session_directory_writes(session)
elif mode in ['act', 'plain', 'hold', 'hold-tree', 'ignore-term', 'freeze-cleanup', 'crash']:
    child = None
    if mode in ['hold-tree', 'ignore-term', 'freeze-cleanup']:
        child = launch_session_child(log, 'call-' + session.name)
        wait(log / ('child-call-' + session.name))
    if mode in ['ignore-term', 'freeze-cleanup']: observe_session_interrupt(log, mode == 'freeze-cleanup')
    record('call', {'directory':path_text(session), 'home':session_home(), 'pid':session_pid(), 'child':child.pid if child else None})
    if mode in ['hold', 'hold-tree', 'ignore-term', 'freeze-cleanup']: wait(log / 'release')
    if mode == 'crash':
        (session / 'exit').write_text('')
        while (log / ('start-' + session.name)).exists() and not (session / 'leader-gone').exists():
            # The second call is issued only after the test observes the leader's exit.
            if (log / 'continue').exists(): break
            time.sleep(0.01)
    text = (session / 'evidence').read_text()
    if mode == 'plain': print(text)
    else: print(json.dumps({'content':[{'type':'text','text':text}]}))
"#;

pub fn script() -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        super::os::session_process_probe(),
        super::os::session_file_holder(),
        super::os::session_interrupt_observer(),
        super::os::session_directory_write_denial(),
        super::os::portable_path_probe(HARNESS)
    )
}

pub fn views(log: &Path, start: &str, stop: &str, action: &str) -> Value {
    let command = super::os::python_program();
    let log = super::os::path_text(log);
    let command = |mode: &str| json!({"command":command, "args":["harness.py", mode, log]});
    let mut tool = command(action);
    tool["description"] = json!("Operate the running app");
    tool["protocol"] = json!(if action == "plain" { "plain" } else { "json" });
    tool["session"] = json!("app");
    json!({"agent_sessions":{"app":{"start":command(start),"stop":command(stop)}}, "agent_tools":{"act":tool}})
}

pub fn records(log: &Path, kind: &str) -> Vec<Value> {
    let prefix = format!("{kind}-");
    fs::read_dir(log)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            entry
                .file_name()
                .to_str()
                .unwrap()
                .starts_with(&prefix)
                .then(|| serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap())
        })
        .collect()
}

pub fn directories(state: &Path) -> Vec<PathBuf> {
    let root = state.join("tool-sessions");
    if !root.exists() {
        return Vec::new();
    }
    fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect()
}

pub fn assert_cleaned(state: &Path, log: &Path, count: usize) {
    assert!(directories(state).is_empty());
    let starts = records(log, "start");
    assert_eq!(starts.len(), count);
    assert_eq!(records(log, "stop").len(), count);
    for start in starts {
        super::os::wait_until(std::time::Duration::from_secs(10), || {
            !super::os::running(start["pid"].as_u64().unwrap() as u32)
                && !super::os::running(start["child"].as_u64().unwrap() as u32)
        });
        assert!(!Path::new(start["directory"].as_str().unwrap()).exists());
        let stopped = records(log, "stop")
            .into_iter()
            .find(|stop| stop["directory"] == start["directory"])
            .unwrap();
        assert_eq!(start["home"], stopped["home"]);
    }
}
