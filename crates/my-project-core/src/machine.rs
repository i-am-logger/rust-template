//! The state machine.

use core::mem;

use my_project_domain::{
    Building, ByteClass, BytePos, Effect, Elapsed, Event, Line, Millis, Pushed, ReadLen, Tally,
    Wire,
};

use crate::effects::{Effects, Queued};

/// What the machine refused, by kind. A refusal is always counted, never
/// silent: the host reads these to know what it lost.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Refusals {
    /// Bytes beyond a line's maximum length.
    pub bytes: Tally,
    /// Effects that found the queue full.
    pub effects: Tally,
    /// Events that arrived after [`Event::End`].
    pub after_end: Tally,
}

impl Refusals {
    const NONE: Self = Self {
        bytes: Tally::ZERO,
        effects: Tally::ZERO,
        after_end: Tally::ZERO,
    };

    /// Every refusal of every kind.
    #[must_use]
    pub const fn total(&self) -> Tally {
        self.bytes
            .saturating_add(self.effects)
            .saturating_add(self.after_end)
    }
}

/// Whether input has ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Open,
    Ended,
}

/// Whether bytes arrived since the last tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Activity {
    Seen,
    Quiet,
}

/// Whether the current quiet spell has been reported.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Idle {
    Pending,
    Reported,
}

/// Counts lines and words in a byte stream, and reports when the stream goes
/// quiet. Holds at most `N` unread effects.
#[derive(Debug)]
pub struct Machine<const N: usize> {
    pos: BytePos,
    line: Line<Building>,
    phase: Phase,
    activity: Activity,
    idle: Idle,
    last_input: Millis,
    idle_after: Elapsed,
    effects: Effects<N>,
    refused: Refusals,
}

impl<const N: usize> Machine<N> {
    /// A machine at the start of a stream, reporting a quiet spell of
    /// `idle_after` or longer.
    #[must_use]
    pub const fn new(idle_after: Elapsed) -> Self {
        Self {
            pos: BytePos::ZERO,
            line: Line::starting_at(BytePos::ZERO),
            phase: Phase::Open,
            activity: Activity::Quiet,
            idle: Idle::Pending,
            last_input: Millis::ZERO,
            idle_after,
            effects: Effects::new(),
            refused: Refusals::NONE,
        }
    }

    /// Takes one event. Never blocks, never allocates, never fails: what it
    /// cannot keep it refuses and counts in [`Machine::refused`].
    pub fn handle(&mut self, event: Event<'_>) {
        if self.phase == Phase::Ended {
            self.refused.after_end = self.refused.after_end.saturating_incr();
            return;
        }
        match event {
            Event::Bytes(wire) => self.bytes(wire),
            Event::Tick(now) => self.tick(now),
            Event::End => self.end(),
        }
    }

    /// The effects owed since the last [`Machine::clear_effects`], oldest
    /// first. Borrowed: the host reads them where they sit.
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        self.effects.as_slice()
    }

    /// Forgets the effects the host has read.
    pub fn clear_effects(&mut self) {
        self.effects.clear();
    }

    /// Everything refused so far.
    #[must_use]
    pub const fn refused(&self) -> Refusals {
        self.refused
    }

    fn bytes(&mut self, wire: Wire<'_>) {
        if !wire.is_empty() {
            self.activity = Activity::Seen;
        }
        for class in wire.classes() {
            self.pos = self.pos.saturating_advance(ReadLen::ONE);
            match class {
                ByteClass::Newline => self.finish_line(),
                other => {
                    if self.line.push(other) == Pushed::Refused {
                        self.refused.bytes = self.refused.bytes.saturating_incr();
                    }
                }
            }
        }
    }

    fn tick(&mut self, now: Millis) {
        match self.activity {
            Activity::Seen => {
                self.last_input = now;
                self.activity = Activity::Quiet;
                self.idle = Idle::Pending;
            }
            Activity::Quiet => {
                if self.idle == Idle::Pending && now.since(self.last_input) >= self.idle_after {
                    self.idle = Idle::Reported;
                    self.emit(Effect::Idle(now));
                }
            }
        }
    }

    fn end(&mut self) {
        if !self.line.is_empty() {
            self.finish_line();
        }
        self.phase = Phase::Ended;
    }

    /// Ends the current line and starts the next at the current position.
    fn finish_line(&mut self) {
        let done = mem::replace(&mut self.line, Line::starting_at(self.pos)).complete();
        self.emit(Effect::Line(done.record()));
    }

    fn emit(&mut self, e: Effect) {
        if self.effects.push(e) == Queued::Dropped {
            self.refused.effects = self.refused.effects.saturating_incr();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines<const N: usize>(m: &Machine<N>) -> usize {
        m.effects()
            .iter()
            .filter(|e| matches!(e, Effect::Line(_)))
            .count()
    }

    #[test]
    fn a_full_queue_drops_and_counts() {
        let mut m: Machine<2> = Machine::new(Elapsed::threshold_ms(10));
        m.handle(Event::Bytes(Wire::of(b"a\nb\nc\n")));
        assert_eq!(lines(&m), 2);
        assert_eq!(m.refused().effects.get(), 1);
        assert_eq!(m.refused().total().get(), 1);
    }

    #[test]
    fn clearing_forgets_what_was_read_and_the_queue_refills_from_empty() {
        let mut m: Machine<2> = Machine::new(Elapsed::threshold_ms(10));
        m.handle(Event::Bytes(Wire::of(b"a\nb\n")));
        assert_eq!(lines(&m), 2);
        m.clear_effects();
        assert!(m.effects().is_empty());
        m.handle(Event::Bytes(Wire::of(b"c\nd\n")));
        assert_eq!(lines(&m), 2);
        assert_eq!(
            m.refused().effects.get(),
            0,
            "a cleared queue has room again"
        );
    }

    #[test]
    fn an_empty_read_is_not_activity() {
        let mut m: Machine<4> = Machine::new(Elapsed::threshold_ms(10));
        m.handle(Event::Bytes(Wire::of(b"x")));
        m.handle(Event::Tick(Millis::from_host(100)));
        m.handle(Event::Bytes(Wire::of(b"")));
        m.handle(Event::Tick(Millis::from_host(200)));
        assert_eq!(m.effects(), &[Effect::Idle(Millis::from_host(200))]);
    }

    #[test]
    fn events_after_the_end_are_refused_and_counted() {
        let mut m: Machine<4> = Machine::new(Elapsed::threshold_ms(10));
        m.handle(Event::End);
        m.handle(Event::Bytes(Wire::of(b"late\n")));
        assert_eq!(lines(&m), 0);
        assert_eq!(m.refused().after_end.get(), 1);
    }

    #[test]
    fn the_last_line_needs_no_newline() {
        let mut m: Machine<4> = Machine::new(Elapsed::threshold_ms(10));
        m.handle(Event::Bytes(Wire::of(b"one two")));
        assert_eq!(lines(&m), 0);
        m.handle(Event::End);
        assert_eq!(lines(&m), 1);
    }

    #[test]
    fn a_quiet_spell_is_reported_once_and_only_after_the_threshold() {
        let mut m: Machine<4> = Machine::new(Elapsed::threshold_ms(10));
        m.handle(Event::Bytes(Wire::of(b"x")));
        m.handle(Event::Tick(Millis::from_host(100)));
        m.handle(Event::Tick(Millis::from_host(109)));
        assert!(m.effects().is_empty(), "9 ms is under the threshold");
        m.handle(Event::Tick(Millis::from_host(110)));
        m.handle(Event::Tick(Millis::from_host(500)));
        assert_eq!(m.effects(), &[Effect::Idle(Millis::from_host(110))]);
    }
}
