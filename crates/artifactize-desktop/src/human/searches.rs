//! Share an in-flight Review query across windows and hints, never a settled cache.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::OnceCell;

use super::search::ReviewPage;
use crate::Failure;

type Reply = Result<ReviewPage, Failure>;
type InFlight = OnceCell<Reply>;
type Queries = BTreeMap<(String, u32), Weak<InFlight>>;

#[derive(Default)]
pub(super) struct Searches(Mutex<Queries>);

impl Searches {
    pub(super) async fn read(
        &self,
        query: String,
        start: u32,
        load: impl Future<Output = Reply>,
    ) -> Reply {
        let pending = {
            let mut in_flight = self.0.lock().expect("Review searches lock");
            in_flight.retain(|_, read| read.strong_count() != 0);
            let entry = in_flight.entry((query, start)).or_default();
            entry.upgrade().unwrap_or_else(|| {
                let created = Arc::new(OnceCell::new());
                *entry = Arc::downgrade(&created);
                created
            })
        };
        pending.get_or_init(|| load).await.clone()
    }
}
