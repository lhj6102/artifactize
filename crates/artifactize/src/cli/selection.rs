//! Parse the exclusive verification selectors before dispatch; file contents are read on use.

use std::path::{Path, PathBuf};

use artifactize_core::{
    project::selection::{Selection, read_selection_file},
    types::{ArtifactName, EvalId},
};
use clap::{ArgMatches, Args, Command, Error, FromArgMatches, error::ErrorKind};

#[derive(Debug)]
pub enum SelectionArgs {
    All,
    Artifact(ArtifactName),
    Eval(EvalId),
    Evals(Vec<EvalId>),
    Artifacts(Vec<ArtifactName>),
    EvalsFile(PathBuf),
    ArtifactsFile(PathBuf),
}

impl SelectionArgs {
    pub(super) fn resolve(self) -> Result<Selection, String> {
        Ok(match self {
            Self::All => Selection::All,
            Self::Artifact(artifact_id) => Selection::Artifact { artifact_id },
            Self::Eval(eval_id) => Selection::Eval { eval_id },
            Self::Evals(eval_ids) => Selection::Evals { eval_ids },
            Self::Artifacts(artifact_ids) => Selection::Artifacts { artifact_ids },
            Self::EvalsFile(path) => Selection::Evals {
                eval_ids: selection_file(&path)?,
            },
            Self::ArtifactsFile(path) => Selection::Artifacts {
                artifact_ids: selection_file(&path)?,
            },
        })
    }
}

#[derive(Debug, Args)]
#[group(multiple = false)]
struct SelectorFlags {
    /// Select one Artifact.
    artifact: Option<Selector>,
    /// Select one qualified Eval ID.
    #[arg(long, value_name = "ID")]
    eval: Option<EvalId>,
    /// Select comma-separated qualified Eval IDs.
    #[arg(long, value_name = "CSV", value_delimiter = ',', num_args = 1)]
    evals: Option<Vec<EvalId>>,
    /// Select comma-separated Artifact names.
    #[arg(long, value_name = "CSV", value_delimiter = ',', num_args = 1)]
    artifacts: Option<Vec<ArtifactName>>,
    /// Read Eval IDs from a JSON array or one ID per line.
    #[arg(long, value_name = "PATH")]
    evals_file: Option<PathBuf>,
    /// Read Artifact names from a JSON array or one ID per line.
    #[arg(long, value_name = "PATH")]
    artifacts_file: Option<PathBuf>,
    /// Select every Artifact.
    #[arg(long)]
    all: bool,
}

impl SelectorFlags {
    fn selected(self) -> Result<Option<SelectionArgs>, Error> {
        let mut selections = [
            self.artifact.map(|selector| match selector {
                Selector::Artifact(id) => SelectionArgs::Artifact(id),
                Selector::Eval(id) => SelectionArgs::Eval(id),
            }),
            self.eval.map(SelectionArgs::Eval),
            self.evals.map(SelectionArgs::Evals),
            self.artifacts.map(SelectionArgs::Artifacts),
            self.evals_file.map(SelectionArgs::EvalsFile),
            self.artifacts_file.map(SelectionArgs::ArtifactsFile),
            self.all.then_some(SelectionArgs::All),
        ]
        .into_iter()
        .flatten();
        let selected = selections.next();
        if selections.next().is_some() {
            return Err(Error::raw(
                ErrorKind::ArgumentConflict,
                "Select only one Artifact or Eval selector.",
            ));
        }
        Ok(selected)
    }
}

impl FromArgMatches for SelectionArgs {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, Error> {
        Ok(SelectorFlags::from_arg_matches(matches)?
            .selected()?
            .unwrap_or(Self::All))
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), Error> {
        if let Some(selected) = SelectorFlags::from_arg_matches(matches)?.selected()? {
            *self = selected;
        }
        Ok(())
    }
}

impl Args for SelectionArgs {
    fn group_id() -> Option<clap::Id> {
        SelectorFlags::group_id()
    }

    fn augment_args(command: Command) -> Command {
        SelectorFlags::augment_args(command)
    }

    fn augment_args_for_update(command: Command) -> Command {
        SelectorFlags::augment_args_for_update(command)
    }
}

/// The positional selection intentionally accepts either an Artifact or qualified Eval.
#[derive(Debug, Clone)]
enum Selector {
    Artifact(ArtifactName),
    Eval(EvalId),
}
impl std::str::FromStr for Selector {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.contains('/') {
            text.parse().map(Self::Eval)
        } else {
            text.parse().map(Self::Artifact)
        }
    }
}
pub(super) fn selection_file<T>(path: &Path) -> Result<Vec<T>, String>
where
    T: std::str::FromStr<Err = String>,
{
    read_selection_file(path)?
        .into_iter()
        .map(|text| text.parse::<T>())
        .collect()
}
