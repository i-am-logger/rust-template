//! The byte classes, as a table built at compile time.
//!
//! A `const fn` fills a 256-entry `static`: it lives in read-only data, needs
//! no initialiser at startup, and a classification is one load. The readable
//! definition ([`reference`]) exists only for the generator and the test,
//! which checks every one of the 256 inputs against it.

/// What a byte means to a line counter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ByteClass {
    /// `\n`: ends a line.
    Newline,
    /// ASCII whitespace other than `\n`: separates words.
    Space,
    /// Anything else: part of a word.
    Other,
}

/// The table. Indexed only through [`ByteClass::of`].
static CLASS: [ByteClass; 256] = build();

/// The readable definition the table is compiled from.
const fn reference(b: u8) -> ByteClass {
    match b {
        b'\n' => ByteClass::Newline,
        b' ' | b'\t' | b'\r' | 0x0b | 0x0c => ByteClass::Space,
        _ => ByteClass::Other,
    }
}

const fn build() -> [ByteClass; 256] {
    let mut t = [ByteClass::Other; 256];
    let mut i = 0;
    while i < 256 {
        // `i < 256`, so the narrowing is exact.
        t[i] = reference(i as u8);
        i += 1;
    }
    t
}

impl ByteClass {
    /// The class of one byte: one table load.
    #[must_use]
    #[inline]
    pub(crate) fn of(b: u8) -> Self {
        CLASS[usize::from(b)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The classes as `core` defines whitespace, written independently of
    /// [`reference`]: comparing the table with the definition it was built
    /// from would pass whatever that definition said. `core`'s
    /// `is_ascii_whitespace` leaves out vertical tab (0x0b), which the class
    /// table counts as space, so it is added here by name.
    fn independent(b: u8) -> ByteClass {
        if b == b'\n' {
            ByteClass::Newline
        } else if b.is_ascii_whitespace() || b == 0x0b {
            ByteClass::Space
        } else {
            ByteClass::Other
        }
    }

    /// Exhaustive: the domain is finite, so every input is checked, and the
    /// count is asserted so a loop that silently ran short cannot pass.
    #[test]
    fn the_table_agrees_with_an_independent_definition_for_every_byte() {
        let mut checked = 0_u32;
        for b in u8::MIN..=u8::MAX {
            assert_eq!(ByteClass::of(b), independent(b), "byte {b:#04x}");
            checked += 1;
        }
        assert_eq!(checked, 256, "the sweep did not cover every byte");
    }
}
