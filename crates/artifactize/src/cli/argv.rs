//! Decode facade options in one pass, then let Clap parse only the command arguments.

use clap::Parser;
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};

use super::Cli;

pub(super) enum ParseError {
    RepeatedGlobal { name: &'static str, json: bool },
    Clap { error: clap::Error, json: bool },
}

#[derive(Default)]
struct Globals {
    repo: Option<PathArg>,
    state_dir: Option<PathArg>,
    json: bool,
    repeated: Option<&'static str>,
}

enum PathArg {
    Attached,
    Separate(PathBuf),
}

impl PathArg {
    fn resolve(self, attached: Option<PathBuf>) -> Option<PathBuf> {
        match self {
            Self::Attached => attached,
            Self::Separate(path) => Some(path),
        }
    }
}

#[derive(Default)]
enum Prefix {
    #[default]
    Root,
    Tools,
    ToolHelp,
    Mcp,
    McpTrailing,
    Other,
    Trailing,
}

impl Prefix {
    fn advance(&mut self, token: &OsStr) {
        if token == "--" {
            *self = if matches!(self, Self::Mcp) {
                Self::McpTrailing
            } else {
                Self::Trailing
            };
        } else if !token.as_encoded_bytes().starts_with(b"-") {
            *self = match (&*self, token.to_str()) {
                (Self::Root, Some("mcp")) => Self::Mcp,
                (Self::Root, Some("tools")) => Self::Tools,
                (Self::Tools, Some("help")) => Self::ToolHelp,
                // After the help program, its remaining arguments are opaque to the facade.
                (Self::ToolHelp, _) => Self::Trailing,
                (Self::Root | Self::Tools, _) => Self::Other,
                _ => return,
            };
        }
    }
}

pub(super) fn parse_args(args: &[OsString]) -> Result<(Cli, bool), ParseError> {
    let mut args = args.iter();
    let mut command = args.next().into_iter().cloned().collect::<Vec<_>>();
    let mut remaining = args.peekable();
    let mut globals = Globals::default();
    let mut prefix = Prefix::default();
    while let Some(token) = remaining.next() {
        if matches!(prefix, Prefix::Trailing | Prefix::McpTrailing) {
            command.push(token.clone());
            continue;
        }
        if token == "--json" {
            if std::mem::replace(&mut globals.json, true) {
                globals.repeated.get_or_insert("--json");
            }
            continue;
        }
        let raw = token.as_encoded_bytes();
        let path = [
            ("--repo", &mut globals.repo),
            ("--state-dir", &mut globals.state_dir),
        ]
        .into_iter()
        .find_map(|(name, slot)| {
            let value = if raw == name.as_bytes() {
                PathArg::Separate(
                    remaining
                        .next_if(|next| !next.as_encoded_bytes().starts_with(b"-"))?
                        .into(),
                )
            } else {
                raw.strip_prefix(name.as_bytes())?.strip_prefix(b"=")?;
                // The native attached value stays intact for Clap's OsString parser.
                command.push(token.clone());
                PathArg::Attached
            };
            Some((name, slot, value))
        });
        if let Some((name, slot, value)) = path {
            if slot.replace(value).is_some() {
                globals.repeated.get_or_insert(name);
            }
            continue;
        }
        prefix.advance(token);
        command.push(token.clone());
    }
    let json = globals.json && !matches!(prefix, Prefix::Mcp | Prefix::McpTrailing);
    if let Some(name) = globals.repeated {
        return Err(ParseError::RepeatedGlobal { name, json });
    }
    let mut cli = Cli::try_parse_from(command).map_err(|error| ParseError::Clap { error, json })?;
    cli.repo = globals.repo.and_then(|repo| repo.resolve(cli.repo));
    cli.state_dir = globals
        .state_dir
        .and_then(|state| state.resolve(cli.state_dir));
    cli.json = json;
    Ok((cli, json))
}
