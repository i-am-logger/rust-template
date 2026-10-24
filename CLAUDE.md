# Working notes

Standing rules for agents in this repo. Records go in `docs/record.md`,
numbers in `MEASUREMENTS.md`, decisions in `docs/decisions.md`; this file is
loaded into every session whole, so it holds rules only and has a byte budget
(`budgets.toml`, checked by `cargo xtask docs`).

The rust-code plugin's skills carry the reasoning: `rust-domain`,
`rust-architecture`, `rust-idioms`, `rust-research`, `rust-measure`, `rust-qa`.

## The pillars are gates

`devenv test` runs every gate; CI runs the same command. A failing gate is
fixed in the code, not by raising a budget: a budget moves only in a commit of
its own that says why.

1. **Type system.** No raw primitive in a domain or core public API outside
   a named seam (`budgets.toml`); no `bool` parameter or field; no public
   newtype field. Illegal programs are `crates/my-project-domain/tests/ui/fail`
   cases, each with its passing twin.
2. **Event-driven.** The core takes `Event`s and queues `Effect`s for the host
   to poll; no callbacks, no clock, no I/O. Time arrives as an event.
3. **Layering.** `layers.toml` gives every crate a role and says what it may
   depend on. Features are additive.
4. **Zero-copy.** Borrow input; no allocation per event
   (`crates/my-project-core/tests/alloc.rs`);
   no `Box::leak`/`mem::forget` -- own, don't leak.
5. **no_std.** Domain and core build for bare-metal targets (`test:bare`).
6. **No unsafe.** `unsafe_code` is forbidden workspace-wide; the only escape
   is a declared quarantine crate.

## How to change things

- Read the governing text first and cite it as `[[source: <key>]]` with a
  clause; pin the source in `docs/research/sources/MANIFEST.toml`.
- A performance claim needs a measurement: `cargo xtask bench`, an A/A arm,
  and ranges that do not overlap. Write "unmeasured" rather than estimate.
- A test that cannot fail is not a test: red first, then green.
- Strike a wrong claim through in place and say what replaced it.
