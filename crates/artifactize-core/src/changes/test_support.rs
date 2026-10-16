//! A subscribed fixture closes IPC before its caller can remove the temporary state.
use super::{Endpoint, Subscription};
use futures_util::FutureExt;
use std::{future::Future, path::Path, pin::Pin};

/// Wait for successful hub ACKs for every publication submitted before this call.
/// Unlike the public best-effort drain, timeout/lost transport is a fixture failure.
pub(crate) async fn delivered(publisher: &super::Publisher) {
    let publishing = publisher.0.as_ref().expect("the fixture has a publisher");
    let target = publishing.pending.lock().unwrap().submitted;
    loop {
        let drained = publishing.drained.notified();
        if publishing.pending.lock().unwrap().delivered >= target {
            break;
        }
        drained.await;
    }
    assert_eq!(
        *publishing.delivery_error.lock().unwrap(),
        None,
        "the fixture requires actual hub Delivered ACKs"
    );
}

struct Namespace(std::path::PathBuf);

impl Drop for Namespace {
    fn drop(&mut self) {
        // This directory belongs only to this fixture state. Unlinking its entries
        // never follows a deliberately unsafe owner.lock symlink into another root.
        let removed = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            removed.expect("remove the fixture's IPC namespace");
        }
    }
}

/// Keep the subscription outside the assertion future so even assertion unwind joins
/// watch/hub/probe shutdown before removing the external namespace or temporary state.
pub(crate) async fn subscribed<T>(
    state: &Path,
    assertions: impl for<'a> FnOnce(&'a mut Subscription) -> Pin<Box<dyn Future<Output = T> + 'a>>,
) -> T {
    let endpoint = Endpoint::new(state).unwrap();
    let _namespace = Namespace(endpoint.directory().to_owned());
    let mut subscription = Subscription::new(state).await;
    let result = std::panic::AssertUnwindSafe(async { assertions(&mut subscription).await })
        .catch_unwind()
        .await;
    let closed = subscription.close().await;
    match result {
        Ok(result) => {
            closed.expect("close the fixture subscription");
            result
        }
        Err(panic) => {
            if let Err(error) = closed {
                eprintln!("Fixture subscription shutdown failed: {error}");
            }
            std::panic::resume_unwind(panic)
        }
    }
}
