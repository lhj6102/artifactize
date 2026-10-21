//! The Human reviews waiting for a person, in every repository on this machine, those of
//! removed branches included: what the pane's waiting stack lists.
use super::{Label, Labels};
use crate::{
    review::search::{self, Entry, Query},
    store::RequestView,
    types::ReviewerId,
};
use std::path::Path;

/// One waiting Human review, with the project and branch its Run ran on.
#[derive(Debug, Clone)]
pub struct Waiting {
    pub view: RequestView,
    pub entry: Entry,
    pub label: Label,
}

/// Every waiting Human review, newest first, as the review search's starting query finds them
/// (a follower listed once, as its original), each named by `labels`, the project and branch
/// of its Run. A review whose Run has no label (its record cannot be read) is named by its
/// repository's folder.
pub async fn waiting(
    state: &Path,
    reviewer: &ReviewerId,
    labels: &Labels,
) -> Result<Vec<Waiting>, String> {
    let query: Query = search::DEFAULT
        .parse()
        .expect("the review search's starting query parses");
    let mut found = Vec::new();
    let mut start = 0;
    loop {
        let page = search::search(state, &query, reviewer, start).await?;
        found.extend(page.requests);
        match page.next {
            Some(next) => start = next,
            None => break,
        }
    }
    found.sort_by(|(a, _), (b, _)| {
        (b.request.created_at, &b.request.id).cmp(&(a.request.created_at, &a.request.id))
    });
    Ok(found
        .into_iter()
        .map(|(view, entry)| {
            let label = labels
                .get(&view.request.run_id)
                .cloned()
                .unwrap_or_else(|| Label {
                    project: Path::new(&entry.repository).file_name().map_or_else(
                        || entry.repository.clone(),
                        |name| name.to_string_lossy().into_owned(),
                    ),
                    branch: String::new(),
                });
            Waiting { view, entry, label }
        })
        .collect())
}
