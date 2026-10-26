//! Pillar 4's hot-path guarantee: handling an event allocates nothing, and a
//! long run does not grow. `dhat` is a safe global allocator, so counting
//! costs no `unsafe` here (pillar 6).
//!
//! One test per binary: dhat allows one profiler per process.

use my_project_core::{Elapsed, Event, Machine, Millis, Wire};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[test]
fn handling_events_allocates_nothing_and_nothing_grows() {
    let input = b"the quick brown fox\njumps over\n\tthe lazy dog\n".repeat(64);
    // Built before the profiler starts: setup may allocate, the hot path may not.
    let mut m: Box<Machine<64>> = Box::new(Machine::new(Elapsed::threshold_ms(5)));

    let _profiler = dhat::Profiler::builder().testing().build();
    let before = dhat::HeapStats::get();
    for round in 0..1_000_u64 {
        m.handle(Event::Tick(Millis::from_host(round)));
        for chunk in input.chunks(7) {
            m.handle(Event::Bytes(Wire::of(chunk)));
            m.clear_effects();
        }
    }
    let after = dhat::HeapStats::get();

    dhat::assert_eq!(after.total_blocks, before.total_blocks);
    dhat::assert_eq!(after.curr_bytes, before.curr_bytes);
}
