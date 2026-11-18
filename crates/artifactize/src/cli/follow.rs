//! Parse the mutually exclusive ways that `verify` follows a submitted Run.

use clap::{ArgMatches, Args, Command, Error, FromArgMatches};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Follow {
    /// Print the Run id and return at once.
    Submit,
    /// Observe until the Run ends, then render its result.
    Wait,
    /// Stream JSON-line updates until the Run ends.
    Watch,
}

#[derive(Args)]
struct Flags {
    /// Observe until the Run ends; interrupting stops watching, not execution.
    #[arg(long)]
    wait: bool,
    /// Submit and stream JSON-line updates, eval results and final validation.
    #[arg(long, conflicts_with = "wait")]
    watch: bool,
}

impl Flags {
    fn selected(self) -> Option<Follow> {
        match (self.wait, self.watch) {
            (false, false) => None,
            (true, false) => Some(Follow::Wait),
            (false, true) => Some(Follow::Watch),
            (true, true) => unreachable!("clap makes --wait and --watch exclusive"),
        }
    }
}

impl FromArgMatches for Follow {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, Error> {
        Ok(Flags::from_arg_matches(matches)?
            .selected()
            .unwrap_or(Self::Submit))
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), Error> {
        if let Some(follow) = Flags::from_arg_matches(matches)?.selected() {
            *self = follow;
        }
        Ok(())
    }
}

impl Args for Follow {
    fn augment_args(command: Command) -> Command {
        Flags::augment_args(command)
    }

    fn augment_args_for_update(command: Command) -> Command {
        Flags::augment_args_for_update(command)
    }
}
