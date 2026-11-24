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
    /// Invalidate dependants before allowing new-generation work to publish.
    pub fn observe(&self, origin: Origin, invalidate: impl FnOnce()) -> u32 {
        let mut seen = self.seen.lock().expect("generation lock");
        match &mut *seen {
            Some((known, generation)) => {
                let replaced = origin.replaces(known);
                *known = origin;
                if replaced {
                    *generation += 1;
                    invalidate();
                }
                *generation
            }
            None => {
                *seen = Some((origin, 0));
                0
            }
        }
    }

    /// Commit a short, synchronous update only in the generation its read captured.
    pub fn if_current<T>(&self, expected: u32, apply: impl FnOnce() -> T) -> Option<T> {
        let seen = self.seen.lock().expect("generation lock");
        let current = seen.as_ref().map_or(0, |(_, generation)| *generation);
        (current == expected).then(apply)
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
