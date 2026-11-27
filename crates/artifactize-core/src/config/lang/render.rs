//! An expanded Artifact written back as `.artf` code: every parameter, default and spread
//! filled in, its toolsets resolved to their names, slots kept as written.

use std::fmt::Write;

use super::{LoweredArtifact, LoweredEval, LoweredFingerprint, Piece, SlotRef, SlotTarget};
use crate::config::{Fingerprint, Profile};

const INDENT: &str = "  ";

pub(crate) fn artifact(artifact: &LoweredArtifact) -> String {
    let mut out = format!("artf {}", artifact.name);
    if !artifact.toolsets.is_empty() {
        write!(out, " : {}", artifact.toolsets.join(" & ")).expect("write to String");
    }
    out.push_str(" = {\n");
    field(&mut out, 1, "path", &quote(&artifact.path.0));
    if let Some(basis) = artifact.basis {
        field(&mut out, 1, "basis", &basis.to_string());
    }
    if !artifact.parts.is_empty() {
        let parts: Vec<_> = artifact.parts.iter().map(|(part, _)| part.as_str()).collect();
        field(&mut out, 1, "parts", &format!("[{}]", parts.join(", ")));
    }
    if let Some((select, _)) = &artifact.select {
        field(&mut out, 1, "select", &strings(select));
    }
    match &artifact.fingerprint.0 {
        LoweredFingerprint::Default => {}
        LoweredFingerprint::Disabled => field(&mut out, 1, "fingerprint", "false"),
        LoweredFingerprint::Declared(fingerprint, _) => {
            field(&mut out, 1, "fingerprint", &render_fingerprint(fingerprint));
        }
    }
    if !artifact.tags.is_empty() {
        field(&mut out, 1, "tags", &strings(&artifact.tags));
    }
    for eval in &artifact.evals {
        out.push('\n');
        render_eval(&mut out, eval);
    }
    out.push_str("}\n");
    out
}

fn field(out: &mut String, depth: usize, key: &str, value: &str) {
    writeln!(out, "{}{key} = {value}", INDENT.repeat(depth)).expect("write to String");
}

fn render_eval(out: &mut String, eval: &LoweredEval) {
    let profile = match (&eval.profile_name, &eval.profile) {
        (Some(name), _) => name.clone(),
        (None, Profile::Human {}) => "human".into(),
        (None, Profile::Runtime { command, args, timeout_ms }) => {
            let args: Vec<_> = args.iter().map(|arg| quote(&arg.to_string())).collect();
            let mut call = format!("runtime(command = {}, args = [{}]", quote(command), args.join(", "));
            if let Some(timeout) = timeout_ms {
                write!(call, ", timeout_ms = {}", timeout.as_millis()).expect("write to String");
            }
            call.push(')');
            call
        }
        (None, Profile::Agent { .. }) => unreachable!("an Agent eval names its profile"),
    };
    writeln!(out, "{INDENT}eval {} : {profile} = {{", eval.id).expect("write to String");
    field(out, 2, "title", &quote(&eval.title));
    if let Some(red_at) = eval.red_at {
        field(out, 2, "red_at", &quote(red_at.as_str()));
    }
    if !eval.mutex.is_empty() {
        let tags: Vec<_> = eval.mutex.iter().map(|tag| tag.as_str().to_owned()).collect();
        field(out, 2, "mutex", &strings(&tags));
    }
    if let Some((pieces, _)) = &eval.instruction {
        field(out, 2, "instruction", &template(pieces, 2));
    }
    if !eval.inputs.is_empty() {
        let inputs: Vec<_> = eval.inputs.iter().map(|(input, _)| input.as_str()).collect();
        field(out, 2, "inputs", &format!("[{}]", inputs.join(", ")));
    }
    writeln!(out, "{INDENT}}}").expect("write to String");
}

fn render_fingerprint(fingerprint: &Fingerprint) -> String {
    match fingerprint {
        Fingerprint::Graph => "none".into(),
        Fingerprint::Artifactsum { files, ignore } => {
            let files: Vec<_> = files.iter().map(|file| file.as_str().to_owned()).collect();
            format!(
                "{{ files = {}, ignore = {} }}",
                strings(&files),
                strings(ignore.patterns())
            )
        }
        Fingerprint::Script {
            command,
            args,
            files,
            timeout_ms,
        } => {
            let args: Vec<_> = args.iter().map(|arg| quote(&arg.to_string())).collect();
            let files: Vec<_> = files.iter().map(|file| file.as_str().to_owned()).collect();
            let mut call = format!(
                "script(command = {}, args = [{}], files = {}",
                quote(command),
                args.join(", "),
                strings(&files)
            );
            if let Some(timeout) = timeout_ms {
                write!(call, ", timeout_ms = {}", timeout.as_millis()).expect("write to String");
            }
            call.push(')');
            call
        }
    }
}

fn strings(values: &[String]) -> String {
    let quoted: Vec<_> = values.iter().map(|value| quote(value)).collect();
    format!("[{}]", quoted.join(", "))
}

/// A one-line string with its escapes; a literal `@{` is `@@{`.
pub(crate) fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    out.push_str(&escape(text, true));
    out.push('"');
    out
}

pub(crate) fn escape(text: &str, one_line: bool) -> String {
    let mut out = String::new();
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' if one_line => out.push_str("\\\""),
            '\n' if one_line => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            control if control.is_control() && control != '\n' => {
                write!(out, "\\u{:04x}", u32::from(control)).expect("write to String");
            }
            other => out.push(other),
        }
    }
    let out = out.replace("@{", "@@{");
    if one_line { out } else { out.replace("\"\"\"", "\\\"\"\"") }
}

/// An instruction: its text and its slots as written, multi-line in `"""` when it reads back
/// the same after dedenting.
pub(crate) fn template(pieces: &[Piece], depth: usize) -> String {
    let text_of = |one_line: bool| -> String {
        pieces
            .iter()
            .map(|piece| match piece {
                Piece::Text(text) => escape(text, one_line),
                Piece::Slot(slot) => slot_text(slot),
            })
            .collect()
    };
    let flat: String = pieces
        .iter()
        .map(|piece| match piece {
            Piece::Text(text) => text.clone(),
            Piece::Slot(_) => "x".into(),
        })
        .collect();
    let indented = flat
        .lines()
        .filter(|line| !line.trim().is_empty())
        .all(|line| line.starts_with([' ', '\t']));
    if !flat.contains('\n') || indented || flat.ends_with('\n') {
        return format!("\"{}\"", text_of(true));
    }
    let indent = INDENT.repeat(depth + 1);
    let body: Vec<String> = text_of(false)
        .split('\n')
        .map(|line| if line.is_empty() { String::new() } else { format!("{indent}{line}") })
        .collect();
    format!("\"\"\"\n{}\n{}\"\"\"", body.join("\n"), INDENT.repeat(depth))
}

fn slot_text(slot: &SlotRef) -> String {
    let target = |target: &SlotTarget| match target {
        SlotTarget::SelfRef => "self".to_owned(),
        SlotTarget::Named(name, _) => name.clone(),
    };
    match slot {
        SlotRef::Artf { target: name, .. } => format!("@{{artf {}}}", target(name)),
        SlotRef::Path { target: name, path, .. } if path.is_empty() => {
            format!("@{{path {}}}", target(name))
        }
        SlotRef::Path { target: name, path, .. } => format!("@{{path {}/{path}}}", target(name)),
        SlotRef::Tool {
            target: name,
            audience,
            tool,
            ..
        } => format!("@{{tool {} {audience} {tool}}}", target(name)),
    }
}
