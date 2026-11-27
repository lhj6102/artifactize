//! Wire names shared by argument decoding and tool discovery.
macro_rules! names {
    ($kind:ident { $($variant:ident => $name:literal),+ $(,)? }) => {
        #[derive(Clone, Copy)]
        pub(super) enum $kind {
            $($variant),+
        }

        impl $kind {
            fn parse(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }

            pub(super) fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }
        }
    };
}

names!(VerifyTool {
    Start => "verify_start",
    Status => "verify_status",
    Wait => "verify_wait",
    Watch => "verify_watch",
    Result => "verify_result",
    Cancel => "verify_cancel",
});
names!(ReadTool {
    Status => "status",
    Graph => "config_graph",
    Logs => "request_logs",
    Config => "config_check",
    Expand => "config_expand",
    Symbols => "config_symbols",
    Runs => "run_list",
    Run => "run_show",
    Requests => "request_list",
    Request => "request_show",
    Session => "session_show",
    Tools => "tools_check",
    Doctor => "doctor",
    Models => "models",
    Remote => "remote_status",
    Cache => "cache_show",
    Plugins => "plugin_list",
});

names!(PluginTool {
    Add => "plugin_add",
    Remove => "plugin_remove",
});

#[derive(Clone, Copy)]
pub(super) enum ToolId {
    Verify(VerifyTool),
    Read(ReadTool),
    Plugin(PluginTool),
}

impl ToolId {
    pub(super) fn parse(name: &str) -> Option<Self> {
        VerifyTool::parse(name)
            .map(Self::Verify)
            .or_else(|| ReadTool::parse(name).map(Self::Read))
            .or_else(|| PluginTool::parse(name).map(Self::Plugin))
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Verify(tool) => tool.name(),
            Self::Read(tool) => tool.name(),
            Self::Plugin(tool) => tool.name(),
        }
    }
}

impl From<VerifyTool> for ToolId {
    fn from(tool: VerifyTool) -> Self {
        Self::Verify(tool)
    }
}

impl From<ReadTool> for ToolId {
    fn from(tool: ReadTool) -> Self {
        Self::Read(tool)
    }
}

impl From<PluginTool> for ToolId {
    fn from(tool: PluginTool) -> Self {
        Self::Plugin(tool)
    }
}
