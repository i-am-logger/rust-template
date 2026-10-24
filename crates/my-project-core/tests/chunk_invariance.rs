//! Pillar 2's property: the effects of an input do not depend on how the host
//! split it into reads, and they agree with an independent model.
//!
//! `cargo xtask arch` requires a test named `chunk_invariance_*` in every core
//! crate. The model below is written against `std`'s own line and word
//! splitting, not against the machine, so a defect shared by the machine and
//! its test cannot pass both.

use my_project_core::{Effect, Elapsed, Event, LineRecord, Machine, Wire};
use proptest::prelude::*;

/// Large enough that no generated input fills it: a dropped effect would make
/// the comparison vacuous, and `refused()` is asserted empty below.
const CAP: usize = 4096;

/// The bytes the generator draws from. `0x0b` is left out because `std`'s
/// `split_ascii_whitespace` does not treat it as a separator while the
/// machine does; that difference is tested by hand in the domain.
const ALPHABET: &[u8] = b"ab \t\r\n";

fn run(chunks: &[&[u8]]) -> Vec<LineRecord> {
    let mut m: Box<Machine<CAP>> = Box::new(Machine::new(Elapsed::threshold_ms(1)));
    for c in chunks {
        m.handle(Event::Bytes(Wire::of(c)));
    }
    m.handle(Event::End);
    let r = m.refused();
    assert_eq!(
        (r.bytes.get(), r.effects.get()),
        (0, 0),
        "nothing may be refused"
    );
    m.effects()
        .iter()
        .map(|e| match e {
            Effect::Line(l) => *l,
            Effect::Idle(_) => panic!("no tick was sent, so no idle can be owed"),
        })
        .collect()
}

/// The independent model: (start, len, words) per line, from `std`.
fn model(input: &[u8]) -> Vec<(u64, u32, u32)> {
    let mut out = Vec::new();
    let mut start = 0_u64;
    let pieces: Vec<&[u8]> = input.split(|&b| b == b'\n').collect();
    let last = pieces.len() - 1;
    for (i, line) in pieces.iter().enumerate() {
        // A final piece with no newline after it is a line only if non-empty.
        if i == last && line.is_empty() {
            break;
        }
        let text = core::str::from_utf8(line).expect("the alphabet is ASCII");
        let words = text.split_ascii_whitespace().count();
        out.push((
            start,
            u32::try_from(line.len()).expect("short"),
            u32::try_from(words).expect("short"),
        ));
        start += u64::try_from(line.len()).expect("short") + 1;
    }
    out
}

fn triples(v: &[LineRecord]) -> Vec<(u64, u32, u32)> {
    v.iter()
        .map(|l| (l.start.get(), l.len.get(), l.words.get()))
        .collect()
}

fn input() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(prop::sample::select(ALPHABET), 0..512)
}

proptest! {
    #[test]
    fn chunk_invariance_any_split_gives_the_same_effects(
        bytes in input(),
        cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..8),
    ) {
        let whole = run(&[&bytes]);
        let mut at: Vec<usize> = cuts.iter().map(|i| i.index(bytes.len() + 1)).collect();
        at.sort_unstable();
        let mut chunks = Vec::new();
        let mut prev = 0;
        for &c in &at {
            chunks.push(&bytes[prev..c]);
            prev = c;
        }
        chunks.push(&bytes[prev..]);
        prop_assert_eq!(run(&chunks), whole);
    }

    #[test]
    fn chunk_invariance_one_byte_reads_agree_with_the_model(bytes in input()) {
        let singles: Vec<&[u8]> = bytes.chunks(1).collect();
        prop_assert_eq!(triples(&run(&singles)), model(&bytes));
    }
}
