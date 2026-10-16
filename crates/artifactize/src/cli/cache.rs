//! Repository-independent reusable result commands.

use super::{Context, print_json};
use serde_json::json;
use std::io::{self, Write};

use super::CacheCommand;

/// Cache show has historically returned 4 for a missing key/history, distinct from usage
/// or storage errors; keep that public code for scripts that probe reusable records.
const MISSING_RECORD_EXIT: u8 = 4;

pub(super) async fn execute(context: Context, command: CacheCommand) -> Result<u8, String> {
    let state = artifactize_core::store::state_dir(context.state_dir.as_deref())?;
    match command {
        CacheCommand::List { history } => {
            let entries = artifactize_core::cache::list(&state, history).await?;
            if context.json {
                print_json(&entries)?;
            } else {
                let mut out = io::stdout().lock();
                writeln!(
                    out,
                    "KEY\tEVAL\tVERDICT\tCOMPLETED\tPRODUCER\tSOURCE\tRECORDS\tBYTES\tLAST USED"
                )
                .map_err(|e| e.to_string())?;
                for entry in entries {
                    writeln!(
                        out,
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        entry.key,
                        entry.eval_id,
                        entry.verdict,
                        entry
                            .completed_at
                            .map_or_else(|| "-".to_owned(), |time| time.to_string()),
                        entry.producer.as_deref().unwrap_or("-"),
                        entry
                            .origin
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| artifactize_core::platform::path_text(
                                &entry.repo_path
                            )),
                        entry.records,
                        entry.bytes,
                        entry.last_used
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        CacheCommand::Show { key, history } => {
            let mut records = artifactize_core::cache::show(&state, &key, history).await?;
            let found = !records.is_empty();
            if history {
                print_json(&records)?;
            } else {
                print_json(&records.pop())?;
            }
            return Ok(if found { 0 } else { MISSING_RECORD_EXIT });
        }
        CacheCommand::Rm { key } => {
            print_json(&json!({"removed": artifactize_core::cache::remove(&state, &key).await?}))?;
        }
    }
    Ok(0)
}
