//! The driver (role: `bin`): the only crate that touches the outside world.
//!
//! It reads stdin, reads the clock, and turns both into [`Event`]s for the
//! core; it prints what the core owes. Everything it does is I/O or
//! translation -- the decisions are the core's, where they can be tested
//! without a terminal, a clock or a file.

use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::time::Instant;

mod marks;

use clap::Parser;
use my_project_core::{Effect, Elapsed, Event, Machine, Millis, Tally, Wire};

/// How many effects the core may owe between drains. A byte owes at most one
/// effect (a newline ends a line), so the driver hands the core at most this
/// many bytes at a time and drains in between: the queue can never overflow,
/// whatever the read size. Feeding a read in slices copies nothing.
const EFFECTS: usize = 1024;

/// Count lines and words on stdin, reporting quiet spells.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Report a quiet spell after this many milliseconds without input.
    #[arg(long, default_value_t = 1000)]
    idle_ms: u64,

    /// Read stdin this many bytes at a time.
    #[arg(long, default_value_t = 64 * 1024)]
    chunk: usize,
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("reading stdin")]
    Read(#[source] io::Error),
    #[error("writing stdout")]
    Write(#[source] io::Error),
}

fn main() -> ExitCode {
    let marks = marks::Marks::start();
    env_logger::init();
    let args = Args::parse();
    marks.mark("args");
    match run(&args, &marks) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!(
                "{e}: {}",
                std::error::Error::source(&e).map_or_else(String::new, ToString::to_string)
            );
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args, marks: &marks::Marks) -> Result<(), Error> {
    let clock = Instant::now();
    let now = || Millis::from_host(u64::try_from(clock.elapsed().as_millis()).unwrap_or(u64::MAX));
    // Boxed once at startup: the machine's queue is a fixed array, and the
    // driver may allocate where the core may not.
    let mut machine: Box<Machine<EFFECTS>> =
        Box::new(Machine::new(Elapsed::threshold_ms(args.idle_ms)));
    let mut buf = vec![0_u8; args.chunk.max(1)];
    let mut stdin = io::stdin().lock();
    let mut out = io::BufWriter::new(io::stdout().lock());
    marks.mark("ready");
    pump(&mut stdin, &mut out, &mut machine, &mut buf, now, marks)?;
    marks.mark("end of input");
    let refused = machine.refused();
    if refused.total() != Tally::ZERO {
        log::warn!("refused: {refused:?}");
    }
    out.flush().map_err(Error::Write)
}

/// Reads `input` to its end, turning each read and the clock into events.
fn pump(
    input: &mut impl Read,
    out: &mut impl Write,
    machine: &mut Machine<EFFECTS>,
    buf: &mut [u8],
    now: impl Fn() -> Millis,
    marks: &marks::Marks,
) -> Result<(), Error> {
    let mut first = true;
    loop {
        let n = input.read(buf).map_err(Error::Read)?;
        machine.handle(Event::Tick(now()));
        drain(machine, out)?;
        if n == 0 {
            machine.handle(Event::End);
            return drain(machine, out);
        }
        for part in buf[..n].chunks(EFFECTS) {
            machine.handle(Event::Bytes(Wire::of(part)));
            drain(machine, out)?;
        }
        if std::mem::take(&mut first) {
            marks.mark("first read");
        }
    }
}

fn drain(machine: &mut Machine<EFFECTS>, out: &mut impl Write) -> Result<(), Error> {
    for e in machine.effects() {
        match e {
            Effect::Line(l) => {
                writeln!(out, "{}\t{}\t{}", l.start.get(), l.len.get(), l.words.get())
            }
            Effect::Idle(at) => writeln!(out, "idle\t{at:?}"),
        }
        .map_err(Error::Write)?;
    }
    machine.clear_effects();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A read larger than the effect queue, of lines short enough that it owes
    /// more effects than the queue holds: every line must still come out.
    #[test]
    fn a_read_that_owes_more_effects_than_the_queue_holds_loses_nothing() {
        let line = b"the quick brown fox jumps over the lazy dog\n";
        let input = line.repeat(23_832);
        let mut machine: Box<Machine<EFFECTS>> =
            Box::new(Machine::new(Elapsed::threshold_ms(1_000)));
        let mut buf = vec![0_u8; 64 * 1024];
        let mut out = Vec::new();
        let marks = marks::Marks::start();
        pump(
            &mut input.as_slice(),
            &mut out,
            &mut machine,
            &mut buf,
            || Millis::ZERO,
            &marks,
        )
        .expect("pump");
        assert_eq!(out.iter().filter(|&&b| b == b'\n').count(), 23_832);
        assert_eq!(machine.refused().total(), Tally::ZERO);
    }
}
