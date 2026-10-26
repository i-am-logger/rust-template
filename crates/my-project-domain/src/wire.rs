//! Bytes off the wire, borrowed: the domain never owns or copies input.

use crate::{ByteClass, ReadLen};

/// One read's bytes, borrowed from the host's buffer (pillar 4: zero-copy).
///
/// The core sees only [`ByteClass`]es, never the raw `u8`s: the one place a
/// byte becomes a meaning is here.
#[derive(Clone, Copy, Debug)]
pub struct Wire<'a>(&'a [u8]);

impl<'a> Wire<'a> {
    /// The entry seam: a host's bytes.
    #[must_use]
    pub const fn of(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }

    /// How many bytes this read holds.
    #[must_use]
    pub const fn len(&self) -> ReadLen {
        ReadLen::of(self.0)
    }

    /// Whether the read is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The bytes' classes, in order. No allocation, no copy.
    pub fn classes(&self) -> impl Iterator<Item = ByteClass> + 'a {
        self.0.iter().map(|&b| ByteClass::of(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_yields_one_class_per_byte_in_order() {
        let w = Wire::of(b"a \n");
        assert!(
            w.classes()
                .eq([ByteClass::Other, ByteClass::Space, ByteClass::Newline])
        );
        assert_eq!(w.len(), ReadLen::of(b"abc"));
    }
}
