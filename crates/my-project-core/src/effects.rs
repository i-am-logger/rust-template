//! A bounded effect queue: a fixed array and a length, no allocation.

use my_project_domain::{Effect, Millis};

/// What a push did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[must_use]
pub(crate) enum Queued {
    /// The effect is queued.
    Taken,
    /// The queue was full; the effect was dropped and must be counted.
    Dropped,
}

/// Up to `N` effects, in order. The host drains it between events.
#[derive(Clone, Debug)]
pub struct Effects<const N: usize> {
    buf: [Effect; N],
    len: Slots,
}

/// How many slots are used. Private: only the queue counts its own slots.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Slots(usize);

/// What fills an unused slot. Never read: [`Effects::as_slice`] stops at the
/// length.
const VACANT: Effect = Effect::Idle(Millis::ZERO);

impl<const N: usize> Effects<N> {
    const NONZERO: () = assert!(N > 0, "an effect queue needs at least one slot");

    /// An empty queue.
    #[must_use]
    pub const fn new() -> Self {
        let () = Self::NONZERO;
        Self {
            buf: [VACANT; N],
            len: Slots(0),
        }
    }

    /// The queued effects, oldest first. Borrowed: nothing is copied.
    #[must_use]
    pub fn as_slice(&self) -> &[Effect] {
        &self.buf[..self.len.0]
    }

    pub(crate) fn push(&mut self, e: Effect) -> Queued {
        match self.buf.get_mut(self.len.0) {
            Some(slot) => {
                *slot = e;
                self.len.0 += 1;
                Queued::Taken
            }
            None => Queued::Dropped,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.len = Slots(0);
    }
}

impl<const N: usize> Default for Effects<N> {
    fn default() -> Self {
        Self::new()
    }
}
