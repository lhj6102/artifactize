//! Share an in-flight Review query across windows and hints, never a settled cache.
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::OnceCell;

use super::search::ReviewPage;
use crate::Failure;

type Reply = Result<ReviewPage, Failure>;
type InFlight = OnceCell<Reply>;
type Queries = BTreeMap<(String, u32), Weak<InFlight>>;

#[derive(Default)]
pub(super) struct Searches {
    pending: Mutex<Queries>,
    // Different queries can still arrive from multiple windows. Keep one database reader.
    reader: tokio::sync::Mutex<()>,
    listed: Mutex<BTreeMap<PathBuf, bool>>,
}

impl Searches {
    pub(super) fn watch(&self, page: &ReviewPage) {
        let mut listed = self.listed.lock().expect("Review repository list lock");
        for review in &page.reviews {
            listed.insert(review.worktree.to_path(), true);
        }
    }

    /// A directory can be removed without any matching Verify or SQLite change.
    pub(crate) fn rescan(&self) -> bool {
        let mut listed = self.listed.lock().expect("Review repository list lock");
        let mut changed = false;
        for (path, exists) in &mut *listed {
            let present = artifactize_core::platform::is_directory(path);
            changed |= present != *exists;
            *exists = present;
        }
        changed
    }

    /// A request following an invalidation must not join an older in-flight page.
    pub(super) fn invalidate(&self) {
        self.pending.lock().expect("Review searches lock").clear();
    }

    pub(super) async fn read(
        &self,
        query: String,
        start: u32,
        load: impl Future<Output = Reply>,
    ) -> Reply {
        let pending = {
            let mut in_flight = self.pending.lock().expect("Review searches lock");
            in_flight.retain(|_, read| read.strong_count() != 0);
            let entry = in_flight.entry((query, start)).or_default();
            entry.upgrade().unwrap_or_else(|| {
                let created = Arc::new(OnceCell::new());
                *entry = Arc::downgrade(&created);
                created
            })
        };
        pending
            .get_or_init(|| async {
                let _reader = self.reader.lock().await;
                load.await
            })
            .await
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::poll_fn, task::Poll};

    #[tokio::test]
    async fn a_refresh_after_invalidation_does_not_reuse_an_in_flight_page() {
        let searches = Searches::default();
        let finish = tokio::sync::Notify::new();
        let mut stale = std::pin::pin!(searches.read("waiting".into(), 0, async {
            finish.notified().await;
            Ok(ReviewPage {
                reviews: vec![],
                next: Some(1),
            })
        }));
        poll_fn(|cx| {
            assert!(stale.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        searches.invalidate();
        let mut fresh = std::pin::pin!(searches.read("waiting".into(), 0, async {
            Ok(ReviewPage {
                reviews: vec![],
                next: None,
            })
        }));
        // The reader serializes database reads, so the fresh read must queue while the
        // stale one is held. Admitting it here makes an incorrect shared result observable.
        poll_fn(|cx| {
            assert!(fresh.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        finish.notify_one();
        let (stale, fresh) = tokio::join!(stale, fresh);
        assert_eq!(stale.unwrap().next, Some(1));
        assert_eq!(fresh.unwrap().next, None);
    }

    #[tokio::test]
    async fn overlapping_searches_share_one_read_but_a_later_search_reads_again() {
        let searches = Searches::default();
        let finish = tokio::sync::Notify::new();
        let mut first = std::pin::pin!(searches.read("waiting".into(), 0, async {
            finish.notified().await;
            Ok(ReviewPage {
                reviews: vec![],
                next: Some(1),
            })
        }));
        let mut second = std::pin::pin!(
            searches.read("waiting".into(), 0, async { panic!("overlapping read") })
        );
        poll_fn(|cx| {
            assert!(first.as_mut().poll(cx).is_pending());
            assert!(second.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        finish.notify_one();
        let (a, b) = tokio::join!(first, second);
        assert_eq!(a.unwrap().next, Some(1));
        assert_eq!(b.unwrap().next, Some(1));
        let later = searches
            .read("waiting".into(), 0, async {
                Ok(ReviewPage {
                    reviews: vec![],
                    next: None,
                })
            })
            .await
            .unwrap();
        assert_eq!(later.next, None);
    }
}
