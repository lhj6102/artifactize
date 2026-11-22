//! Draft writes and verdict submission share the original request's FIFO, across windows.
use std::{
    collections::{BTreeMap, BTreeSet},
    future::{Future, poll_fn},
    sync::{Arc, Mutex, Weak},
    task::Poll,
};

use artifactize_core::types::RequestId;
use tokio::sync::{Mutex as Gate, OwnedMutexGuard};

use crate::Failure;

#[derive(Default)]
pub(super) struct DraftQueue {
    /// Resolve aliases and enqueue in admission order before letting the next command resolve.
    admit: Gate<()>,
    owners: Mutex<BTreeMap<RequestId, Weak<Gate<()>>>>,
}

pub(crate) struct Turns {
    pub(crate) owners: Vec<RequestId>,
    _held: Vec<OwnedMutexGuard<()>>,
}

impl DraftQueue {
    pub(super) async fn enter(
        &self,
        resolve: impl Future<Output = Result<Vec<RequestId>, Failure>>,
    ) -> Result<Turns, Failure> {
        let admission = self.admit.lock().await;
        let owners: Vec<_> = resolve
            .await?
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let gates: Vec<_> = {
            let mut gates = self.owners.lock().expect("draft queues lock");
            gates.retain(|_, gate| gate.strong_count() != 0);
            owners
                .iter()
                .map(|owner| {
                    let slot = gates.entry(owner.clone()).or_default();
                    slot.upgrade().unwrap_or_else(|| {
                        let gate = Arc::new(Gate::new(()));
                        *slot = Arc::downgrade(&gate);
                        gate
                    })
                })
                .collect()
        };
        let mut held = Vec::new();
        let mut waiting = Vec::new();
        for gate in gates {
            let mut turn = Box::pin(gate.lock_owned());
            // Tokio's fair mutex records the waiter on the first poll. Do this before
            // unlocking admission, but never hold admission while another review writes.
            match poll_fn(|cx| Poll::Ready(turn.as_mut().poll(cx))).await {
                Poll::Ready(guard) => held.push(guard),
                Poll::Pending => waiting.push(turn),
            }
        }
        drop(admission);
        for turn in waiting {
            held.push(turn.await);
        }
        Ok(Turns {
            owners,
            _held: held,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn submission_waits_for_an_already_admitted_edit_of_its_original_request() {
        let queue = DraftQueue::default();
        let owner: RequestId = "run-owner-1".parse().unwrap();
        let other: RequestId = "run-other-1".parse().unwrap();
        let saved = std::sync::Mutex::new("previous text");
        // The follower's resolution is in flight while another window tries to submit
        // the original. The two references name the same queue after resolution.
        let resolved = tokio::sync::Notify::new();
        let mut edit = Box::pin(queue.enter(async {
            resolved.notified().await;
            Ok(vec![owner.clone()])
        }));
        assert!(poll_fn(|cx| Poll::Ready(edit.as_mut().poll(cx).is_pending())).await);
        let mut submit = Box::pin(queue.enter(async { Ok(vec![owner.clone()]) }));
        assert!(poll_fn(|cx| Poll::Ready(submit.as_mut().poll(cx).is_pending())).await);
        resolved.notify_one();
        let edit = edit.await.unwrap();
        assert!(poll_fn(|cx| Poll::Ready(submit.as_mut().poll(cx).is_pending())).await);
        let independent = queue
            .enter(async { Ok(vec![other.clone()]) })
            .await
            .unwrap();
        assert_eq!(independent.owners, [other]);
        *saved.lock().unwrap() = "text from the pet";
        drop(edit);
        let submit = submit.await.unwrap();
        assert_eq!(submit.owners, [owner]);
        assert_eq!(*saved.lock().unwrap(), "text from the pet");
    }
}
