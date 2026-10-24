//! A line's lifecycle as typestate: building, then complete, one way.

use core::marker::PhantomData;

use crate::{ByteClass, ByteLen, BytePos, LineRecord, WordCount};

mod sealed {
    /// Sealed, so no other crate can add a state.
    pub trait State {}
}

/// A line still taking bytes. Uninhabited: it exists only as a type.
#[derive(Debug)]
pub enum Building {}
/// A line that has ended. Uninhabited: it exists only as a type.
#[derive(Debug)]
pub enum Complete {}

impl sealed::State for Building {}
impl sealed::State for Complete {}

/// What happened to a pushed byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[must_use]
pub enum Pushed {
    /// The byte is part of the line.
    Taken,
    /// The line is at its maximum length; the byte was refused. The caller
    /// counts it -- a refusal is never silent.
    Refused,
}

/// A line, in state `S`.
#[derive(Debug)]
pub struct Line<S: sealed::State> {
    start: BytePos,
    len: ByteLen,
    words: WordCount,
    prev: ByteClass,
    state: PhantomData<S>,
}

impl Line<Building> {
    /// A new, empty line starting at `start`.
    #[must_use]
    pub const fn starting_at(start: BytePos) -> Self {
        Self {
            start,
            len: ByteLen::ZERO,
            words: WordCount::ZERO,
            prev: ByteClass::Space,
            state: PhantomData,
        }
    }

    /// Takes one non-newline byte's class.
    pub fn push(&mut self, class: ByteClass) -> Pushed {
        let Some(len) = self.len.checked_grow() else {
            return Pushed::Refused;
        };
        self.len = len;
        if class == ByteClass::Other && self.prev != ByteClass::Other {
            self.words = self.words.saturating_incr();
        }
        self.prev = class;
        Pushed::Taken
    }

    /// Whether no byte has been taken yet.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len.get() == 0
    }

    /// Ends the line. Consumes it: a completed line cannot take more bytes.
    #[must_use]
    pub const fn complete(self) -> Line<Complete> {
        Line {
            start: self.start,
            len: self.len,
            words: self.words,
            prev: self.prev,
            state: PhantomData,
        }
    }
}

impl Line<Complete> {
    /// The line as plain old data, for the host.
    #[must_use]
    pub const fn record(&self) -> LineRecord {
        LineRecord {
            start: self.start,
            len: self.len,
            words: self.words,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_counted_at_their_first_byte() {
        let mut l = Line::starting_at(BytePos::ZERO);
        for c in [
            ByteClass::Other,
            ByteClass::Other,
            ByteClass::Space,
            ByteClass::Space,
            ByteClass::Other,
        ] {
            assert_eq!(l.push(c), Pushed::Taken);
        }
        let r = l.complete().record();
        assert_eq!(r.words.get(), 2);
        assert_eq!(r.len.get(), 5);
    }

    #[test]
    fn a_full_line_refuses_and_keeps_its_length() {
        let mut l = Line::starting_at(BytePos::ZERO);
        l.len = ByteLen(u32::MAX);
        assert_eq!(l.push(ByteClass::Other), Pushed::Refused);
        assert_eq!(l.complete().record().len.get(), u32::MAX);
    }
}
