//! The domain as a type system (role: `domain`).
//!
//! Every rule here is enforced by the compiler or by a gate, not by a comment:
//! the wrong program does not build, or `devenv test` fails.
//!
//! * **Opaque quantities.** A [`BytePos`] is not a [`ByteLen`] is not a
//!   [`WordCount`]; none exposes its integer except through one named seam
//!   (`get()`), and there is no arithmetic between them that is not declared.
//! * **Constructor narrowing.** An [`Elapsed`] can only come from
//!   [`Millis::since`], and a [`ByteLen`] only from counting real bytes, so a
//!   value of either type is a proof of how it was made.
//! * **Typestate.** A [`Line`] is `Building` until it is completed; `push`
//!   does not exist on a complete line, and `record` does not exist on one
//!   still being built.
//! * **Two-state enums, never `bool`.** [`Pushed`] says what happened.
//! * **The vocabulary is closed.** Input is an [`Event`], output an
//!   [`Effect`]; both are enums whose payloads are already-narrowed types.
//! * **Zero-copy layouts.** [`LineRecord`] is plain old data: `zerocopy`
//!   derives let a host write it to a pipe, a file or shared memory as bytes,
//!   with no `unsafe` here or there.
//!
//! `no_std`, `forbid(unsafe_code)`, and no dependency but `zerocopy`
//! (`layers.toml`).

#![no_std]

mod class;
mod event;
mod line;
mod quantity;
mod wire;

pub use class::ByteClass;
pub use event::{Effect, Event, LineRecord};
pub use line::{Building, Complete, Line, Pushed};
pub use quantity::{ByteLen, BytePos, Elapsed, Millis, ReadLen, Tally, WordCount};
pub use wire::Wire;
