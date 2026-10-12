//! A portable Python harness with file handshakes, not startup sleeps. Logs live
//! outside the session so tests can inspect cleanup after artifactize removes it.

use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const HARNESS: &str = r#"import json, os, pathlib, subprocess, sys, time
mode, log = sys.argv[1], pathlib.Path(sys.argv[2])
def wait(path):
    while not path.exists() and log.exists(): time.sleep(0.01)
def record(kind, data):
    pending = log / ('.pending-' + kind + '-' + session.name)
    pending.write_text(json.dumps(data))
    pending.replace(log / (kind + '-' + session.name))
if mode == 'child':
    held = hold_session_file(pathlib.Path(os.environ['ARTIFACTIZE_SESSION_DIR']) / ('held-' + sys.argv[3]))
    (log / ('child-' + sys.argv[3])).write_text(str(os.getpid()))
    wait(log / 'release')
    sys.exit(0)
request = json.load(sys.stdin) if mode not in ['plain'] else None
session = pathlib.Path(os.environ['ARTIFACTIZE_SESSION_DIR'])
if mode in ['start', 'unready', 'exit', 'stop', 'hang-stop', 'fail-stop']:
    assert request['version'] == 1 and request['args'] == {}
    assert isinstance(request['context']['scope'], dict)
if mode in ['start', 'unready', 'exit']:
    child = subprocess.Popen([sys.executable, __file__, 'child', str(log), session.name])
    wait(log / ('child-' + session.name))
    (session / 'evidence').write_text('started first')
    record('start', {'directory':str(session), 'pid':os.getpid(), 'child':child.pid, 'home':str(pathlib.Path.home())})
    if mode == 'exit': sys.exit(1)
    if mode == 'start': print('ready', flush=True)
    wait(session / 'exit')
elif mode in ['stop', 'hang-stop', 'fail-stop']:
    child = None
    if mode == 'hang-stop':
        child = subprocess.Popen([sys.executable, __file__, 'child', str(log), 'stop-' + session.name])
        wait(log / ('child-stop-' + session.name))
    record('stop', {'directory':str(session), 'home':str(pathlib.Path.home()), 'pid':os.getpid(), 'child':child.pid if child else None})
    if mode == 'hang-stop': wait(log / 'release')
    if mode == 'fail-stop': sys.exit(1)
elif mode in ['act', 'plain', 'hold', 'hold-tree', 'crash']:
    child = None
    if mode == 'hold-tree':
        child = subprocess.Popen([sys.executable, __file__, 'child', str(log), 'call-' + session.name])
        wait(log / ('child-call-' + session.name))
    record('call', {'directory':str(session), 'pid':os.getpid(), 'child':child.pid if child else None})
    if mode in ['hold', 'hold-tree']: wait(log / 'release')
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
        "{}
{}",
        super::os::session_file_holder(),
        HARNESS
    )
}

pub fn views(log: &Path, start: &str, stop: &str, action: &str) -> Value {
    let command = super::os::python_program();
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
