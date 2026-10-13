//! Which state the shown snapshots, open reviews and claims belong to. Deleting and recreating
//! the state, restoring a backup over its database or pointing at another state replaces it:
//! everything read from the old one stops applying, while ordinary refreshes keep drafts and
//! caches. Replacement is the monitor's own rule, [`Origin::replaces`].
use artifactize_core::monitor::Origin;
use std::sync::Mutex;

#[derive(Default)]
pub struct Generation {
    /// The origin last seen and its generation number.
    seen: Mutex<Option<(Origin, u32)>>,
}

impl Generation {
    /// Note `origin`; the generation advances when it replaces the last one seen.
    /// Returns the generation and whether this observation replaced an earlier state.
    pub fn observe(&self, origin: Origin) -> (u32, bool) {
        let mut seen = self.seen.lock().expect("generation lock");
        match &mut *seen {
            Some((known, generation)) => {
                let replaced = origin.replaces(known);
                *known = origin;
                if replaced {
                    *generation += 1;
                }
                (*generation, replaced)
            }
            None => {
                *seen = Some((origin, 0));
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
