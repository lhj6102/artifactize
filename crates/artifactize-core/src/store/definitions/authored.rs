//! Snapshot authored declarations without round-tripping their typed fields through JSON.
use super::*;

impl From<&config::Profile> for Profile {
    fn from(source: &config::Profile) -> Self {
        Self {
            known: source.into(),
            extra: Map::new(),
        }
    }
}

impl From<&config::EvalDeclaration> for Declaration {
    fn from(source: &config::EvalDeclaration) -> Self {
        Self {
            mutex: nonempty(&source.mutex),
            id: Field::Value(source.id.clone()),
            title: Field::Value(source.title.clone()),
            profile: Field::Value(source.profile().into()),
            profile_name: optional(source.profile_name().map(str::to_owned)),
            profile_variants: Field::Missing,
            payload: Field::Value(source.payload().into()),
            inputs: nonempty(source.inputs()),
            red_at: optional(source.red_at()),
            pass_schema: Field::Missing,
            fail_schema: Field::Missing,
            extra: Map::new(),
        }
    }
}

fn nonempty<T: Clone>(values: &[T]) -> Field<Vec<T>> {
    if values.is_empty() {
        Field::Missing
    } else {
        Field::Value(values.to_vec())
    }
}

fn optional<T>(value: Option<T>) -> Field<T> {
    value.map_or(Field::Missing, Field::Value)
}

impl From<&config::Artifact> for Artifact {
    fn from(source: &config::Artifact) -> Self {
        let mut extra = Map::new();
        if let Some(covering) = &source.covering_part {
            extra.insert("coveringPart".into(), Value::String(covering.to_string()));
        }
        if !source.parts.is_empty() {
            extra.insert("parts".into(), serde_json::json!(source.parts));
        }
        Self {
            definition: Field::Value(source.definition.clone()),
            kind: Field::Value(source.kind()),
            path: Field::Value(source.path.clone()),
            select: nonempty(source.select()),
            nested: Field::Value(source.nested.clone()),
            name: Field::Value(source.name.clone()),
            tags: Field::Value(source.tags.clone()),
            views: Field::Value((&source.views).into()),
            basis: source.basis.into(),
            fingerprint: source.fingerprint.as_ref().map(Fingerprint::from).into(),
            extra,
        }
    }
}

impl From<&config::Views> for Views {
    fn from(source: &config::Views) -> Self {
        Self {
            agent_tools: Field::Value(
                source
                    .agent_tools
                    .iter()
                    .map(|(name, tool)| (name.clone(), tool.into()))
                    .collect(),
            ),
            human_tools: Field::Value(
                source
                    .human_tools
                    .iter()
                    .map(|(name, tool)| (name.clone(), tool.into()))
                    .collect(),
            ),
            extra: Map::from_iter([(
                "agentSessions".into(),
                serde_json::json!(source.agent_sessions),
            )]),
        }
    }
}

impl From<&config::AgentTool> for AgentTool {
    fn from(source: &config::AgentTool) -> Self {
        match source {
            config::AgentTool::Builtin(tool) => Self::Builtin(BuiltinTool {
                builtin: tool.builtin,
                args: optional(tool.args.clone()),
                description: optional(tool.description.clone()),
                extra: Map::new(),
            }),
            config::AgentTool::Command(tool) => Self::Command(CommandTool {
                description: tool.description.clone(),
                input_schema: InputSchema(tool.input_schema.clone()),
                protocol: tool.protocol,
                command: crate::platform::program_text(&tool.command),
                args: tool.args.iter().map(ToString::to_string).collect(),
                timeout_ms: optional(tool.timeout_ms),
                execution_paths: nonempty(&tool.execution_paths),
                extra: tool
                    .session
                    .iter()
                    .map(|session| ("session".into(), Value::String(session.to_string())))
                    .collect(),
            }),
        }
    }
}

impl From<&config::HumanTool> for HumanTool {
    fn from(source: &config::HumanTool) -> Self {
        match source {
            config::HumanTool::Viewer(tool) => Self::Viewer(super::HumanViewerTool {
                kind: tool.kind,
                description: tool.description.clone(),
                plugin: Some(tool.plugin.clone()),
            }),
            config::HumanTool::Builtin(tool) => Self::Builtin(HumanBuiltinTool {
                builtin: tool.builtin,
                description: tool.description.clone(),
                kind: tool.kind(),
                args: tool.args.iter().map(ToString::to_string).collect(),
                extra: Map::new(),
            }),
            config::HumanTool::Command(tool) => Self::Command(HumanCommandTool {
                description: tool.description.clone(),
                kind: tool.kind,
                command: crate::platform::program_text(&tool.command),
                args: tool.args.iter().map(ToString::to_string).collect(),
                extra: tool
                    .timeout_ms
                    .iter()
                    .map(|duration| ("timeoutMs".into(), serde_json::json!(duration.as_millis())))
                    .collect(),
            }),
        }
    }
}

impl From<&config::Fingerprint> for Fingerprint {
    fn from(source: &config::Fingerprint) -> Self {
        match source {
            config::Fingerprint::Graph => Self {
                extra: Map::from_iter([("graph".into(), Value::Bool(true))]),
                ..Default::default()
            },
            config::Fingerprint::Script {
                command,
                args,
                files,
                timeout_ms,
            } => Self {
                script: Field::Value(Script {
                    command: crate::platform::program_text(command),
                    args: args.iter().map(ToString::to_string).collect(),
                    files: Field::Value(files.clone()),
                    timeout_ms: (*timeout_ms).into(),
                    extra: Map::new(),
                }),
                ..Default::default()
            },
            config::Fingerprint::Artifactsum { files, ignore } => Self {
                files: Field::Value(files.clone()),
                ignore: Field::Value(ignore.clone().into()),
                ..Default::default()
            },
        }
    }
}
