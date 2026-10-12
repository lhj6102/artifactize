//! Which state the shown snapshots, open reviews and claims belong to. Deleting and
//! recreating the state (or pointing at another) gives it a new identity: everything read
//! from the old one stops applying, while ordinary refreshes keep drafts and caches.
use artifactize::types::StateId;
use std::sync::Mutex;

#[derive(Default)]
pub struct Generation {
    /// The identity last seen (`None` inside: no database yet) and its generation number.
    seen: Mutex<Option<(Option<StateId>, u32)>>,
}

impl Generation {
    /// Note `identity`; the generation advances when it differs from the last one seen.
    /// Returns the generation and whether this observation replaced an earlier state.
    pub fn observe(&self, identity: Option<StateId>) -> (u32, bool) {
        let mut seen = self.seen.lock().expect("generation lock");
        match &mut *seen {
            Some((known, generation)) if *known == identity => (*generation, false),
            Some((known, generation)) => {
                *known = identity;
                *generation += 1;
                (*generation, true)
            }
            None => {
                *seen = Some((identity, 0));
                (0, false)
            }
        }
    }

    /// The generation last observed.
    pub fn current(&self) -> u32 {
        self.seen
            .lock()
            .expect("generation lock")
            .as_ref()
            .map_or(0, |(_, generation)| *generation)
    }
}
