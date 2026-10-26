//! The machine (role: `core`): events in, effects out, and nothing else.
//!
//! [`Machine::handle`] takes one [`Event`]; what the machine owes the host is
//! queued and read back with [`Machine::effects`] -- polled, never called back
//! (pillar 2). There is no clock, no I/O and no allocation here: time arrives as
//! [`Event::Tick`], bytes as borrowed [`Wire`]s, and every buffer is a fixed
//! array whose overflow is refused and counted, never grown.
//!
//! The state between events lives on the struct, so the effects of an input do
//! not depend on how the host split it into reads; `tests/chunk_invariance.rs`
//! holds that as a property.
//!
//! `no_std`, `forbid(unsafe_code)`, and no dependency but the domain
//! (`layers.toml`).

#![no_std]

mod effects;
mod machine;

pub use effects::Effects;
pub use machine::{Machine, Refusals};

/// The vocabulary, re-exported once so a host depends on one crate.
pub use my_project_domain::{Effect, Elapsed, Event, LineRecord, Millis, Tally, Wire};
