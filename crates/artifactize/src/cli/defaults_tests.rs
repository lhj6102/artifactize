//! Named defaults retain the public CLI/API values.
use super::*;

#[test]
fn default_concurrency_and_run_history_page_remain_compatible() {
    let cli = Cli::try_parse_from(["artifactize", "verify", "--all"]).unwrap();
    let Some(Command::Verify { jobs, .. }) = cli.command else {
        panic!("verify command");
    };
    assert_eq!(jobs, 4);
    assert_eq!(jobs as usize, crate::project::VerifyOptions::default().jobs);
    let cli = Cli::try_parse_from(["artifactize", "run", "list"]).unwrap();
    let Some(Command::Run {
        command: RunCommand::List { limit, offset, .. },
    }) = cli.command
    else {
        panic!("run list command");
    };
    assert_eq!((limit, offset), (50, 0));
}
