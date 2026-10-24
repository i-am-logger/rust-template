//! The measurement instruments (method pillar M).
//!
//! * `cargo xtask arms <rev> <rev> [--bin <name>]` builds each revision from
//!   `git archive` at **one** path (a worktree elsewhere moves code layout
//!   and with it the timing), then compares the machine code before anyone
//!   times anything: byte-identical text is neutral, proven, with no timing.
//! * `cargo xtask bench --arm name=<binary> ... [--aa]` launches each arm,
//!   round after round with the order rotated and the first round dropped,
//!   and reports medians with their ranges: if two ranges overlap there is no
//!   result. `--aa` adds a copy of the first arm, whose spread against it is
//!   the noise any claimed difference must clear.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};

/// One arm's samples, per metric: `wall` and each startup stage.
#[derive(Debug)]
struct Arm {
    name: String,
    path: PathBuf,
    samples: Vec<(String, Vec<f64>)>,
}

impl Arm {
    fn push(&mut self, metric: &str, v: f64) {
        match self.samples.iter_mut().find(|(m, _)| m == metric) {
            Some((_, vs)) => vs.push(v),
            None => self.samples.push((metric.to_string(), vec![v])),
        }
    }

    fn stats(&self, metric: &str) -> Option<Stats> {
        self.samples
            .iter()
            .find(|(m, _)| m == metric)
            .map(|(_, v)| Stats::of(v))
    }
}

/// Median and range of a metric's kept samples.
#[derive(Clone, Copy, Debug)]
struct Stats {
    median: f64,
    min: f64,
    max: f64,
}

impl Stats {
    fn of(v: &[f64]) -> Self {
        let mut s = v.to_vec();
        s.sort_by(f64::total_cmp);
        let mid = s.len() / 2;
        let median = if s.len() % 2 == 0 && mid > 0 {
            (s[mid - 1] + s[mid]) / 2.0
        } else {
            s[mid]
        };
        Self {
            median,
            min: s[0],
            max: s[s.len() - 1],
        }
    }

    fn overlaps(self, o: Self) -> bool {
        self.min <= o.max && o.min <= self.max
    }
}

/// What a difference between two arms means.
fn verdict(base: Stats, arm: Stats, floor: f64) -> String {
    let d = arm.median - base.median;
    if base.overlaps(arm) {
        format!("overlap: no result ({d:+.2} ms)")
    } else if d.abs() < floor {
        format!("disjoint but under the {floor} ms floor: revert ({d:+.2} ms)")
    } else if d < 0.0 {
        format!("disjoint: faster by {:.2} ms", -d)
    } else {
        format!("disjoint: slower by {d:.2} ms")
    }
}

#[derive(Debug, Default)]
struct BenchArgs {
    arms: Vec<(String, PathBuf)>,
    aa: bool,
    rounds: usize,
    floor: f64,
    stdin: Option<PathBuf>,
    cold: Vec<PathBuf>,
    battery: bool,
    args: Vec<String>,
}

fn parse(args: &[String]) -> Result<BenchArgs> {
    let mut b = BenchArgs {
        rounds: 7,
        floor: 1.0,
        ..BenchArgs::default()
    };
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        let mut val = || it.next().with_context(|| format!("{a} needs a value"));
        match a.as_str() {
            "--arm" => {
                let v = val()?;
                let (n, p) = v.split_once('=').context("--arm takes name=path")?;
                b.arms.push((n.to_string(), PathBuf::from(p)));
            }
            "--aa" => b.aa = true,
            "--battery" => b.battery = true,
            "--rounds" => b.rounds = val()?.parse()?,
            "--floor-ms" => b.floor = val()?.parse()?,
            "--stdin" => b.stdin = Some(PathBuf::from(val()?)),
            "--cold" => b.cold.push(PathBuf::from(val()?)),
            "--" => {
                b.args = it.cloned().collect();
                break;
            }
            other => bail!("unknown bench option `{other}`"),
        }
    }
    ensure!(
        !b.arms.is_empty(),
        "usage: cargo xtask bench --arm name=<binary> [--arm ...] [--aa] [--rounds N] [--floor-ms F] [--stdin FILE] [--cold DIR] [-- args]"
    );
    ensure!(
        b.rounds >= 3,
        "at least 3 rounds: the first is dropped, and one sample has no range"
    );
    Ok(b)
}

/// What powers the host, and whether that is mains.
fn power() -> (String, bool) {
    let mac = Command::new("pmset").args(["-g", "batt"]).output();
    if let Ok(out) = mac {
        let s = String::from_utf8_lossy(&out.stdout).to_string();
        let ac = s.contains("AC Power");
        return (if ac { "AC".into() } else { "battery".into() }, ac);
    }
    match fs::read_to_string("/sys/class/power_supply/AC/online") {
        Ok(s) if s.trim() == "0" => ("battery".into(), false),
        Ok(_) => ("AC".into(), true),
        Err(_) => ("unknown (no battery found)".into(), true),
    }
}

fn host() -> String {
    Command::new("uname").arg("-srm").output().map_or_else(
        |_| "unknown".into(),
        |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
    )
}

/// Launches one arm once; returns the wall time and each startup mark.
fn launch(arm: &Path, b: &BenchArgs, marks_env: &str) -> Result<(f64, Vec<(String, f64)>)> {
    let aside: Vec<(PathBuf, PathBuf)> = b
        .cold
        .iter()
        .map(|d| (d.clone(), d.with_extension("cold-aside")))
        .collect();
    for (d, a) in &aside {
        if d.exists() {
            fs::rename(d, a).with_context(|| format!("moving {} aside", d.display()))?;
        }
    }
    let stdin = match &b.stdin {
        Some(p) => {
            Stdio::from(fs::File::open(p).with_context(|| format!("opening {}", p.display()))?)
        }
        None => Stdio::null(),
    };
    let t0 = Instant::now();
    let out = Command::new(arm)
        .args(&b.args)
        .env(marks_env, "1")
        .stdin(stdin)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    let wall = t0.elapsed().as_secs_f64() * 1e3;
    for (d, a) in &aside {
        if a.exists() {
            fs::rename(a, d).with_context(|| format!("restoring {}", d.display()))?;
        }
    }
    let out = out.with_context(|| format!("launching {}", arm.display()))?;
    ensure!(
        out.status.success(),
        "{} exited {}: a failing arm measures nothing",
        arm.display(),
        out.status
    );
    let marks = String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("startup ")?;
            let (ms, stage) = rest.split_once(' ')?;
            Some((stage.to_string(), ms.parse().ok()?))
        })
        .collect();
    Ok((wall, marks))
}

pub(crate) fn bench(args: &[String], root: &Path, marks_env: &str) -> Result<ExitCode> {
    let b = parse(args)?;
    let (power, ac) = power();
    ensure!(
        ac || b.battery,
        "on battery: timings on battery are not comparable with mains -- plug in, or pass --battery and say so in the row"
    );
    let mut arms: Vec<Arm> = b
        .arms
        .iter()
        .map(|(n, p)| Arm {
            name: n.clone(),
            path: p.clone(),
            samples: vec![],
        })
        .collect();
    if b.aa {
        let base = &arms[0];
        let dir = root.join("target/bench");
        fs::create_dir_all(&dir)?;
        let copy = dir.join(format!("{}-aa", base.name));
        fs::copy(&base.path, &copy)
            .with_context(|| format!("copying {} for the A/A arm", base.path.display()))?;
        arms.push(Arm {
            name: format!("{} (A/A)", base.name),
            path: copy,
            samples: vec![],
        });
    }
    for round in 0..b.rounds {
        let n = arms.len();
        for k in 0..n {
            let i = (k + round) % n;
            let (wall, marks) = launch(&arms[i].path, &b, marks_env)?;
            if round > 0 {
                arms[i].push("wall", wall);
                for (stage, ms) in marks {
                    arms[i].push(&stage, ms);
                }
            }
        }
    }
    report(&arms, &b, &power)?;
    Ok(ExitCode::SUCCESS)
}

fn report(arms: &[Arm], b: &BenchArgs, power: &str) -> Result<()> {
    let mut out = std::io::stdout().lock();
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map_or_else(
            |_| "?".into(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
        );
    writeln!(
        out,
        "host {} | power {power} | commit {commit} | {} rounds, first dropped, order rotated | median [min-max] ms",
        host(),
        b.rounds
    )?;
    writeln!(out)?;
    let metrics: Vec<String> = arms[0].samples.iter().map(|(m, _)| m.clone()).collect();
    write!(out, "| metric |")?;
    for a in arms {
        write!(out, " {} |", a.name)?;
    }
    writeln!(out)?;
    writeln!(out, "|---|{}", "---|".repeat(arms.len()))?;
    for m in &metrics {
        write!(out, "| {m} |")?;
        for a in arms {
            match a.stats(m) {
                Some(s) => write!(out, " {:.2} [{:.2}-{:.2}] |", s.median, s.min, s.max)?,
                None => write!(out, " -- |")?,
            }
        }
        writeln!(out)?;
    }
    writeln!(out)?;
    let base = arms[0].stats("wall").context("no wall samples")?;
    for a in &arms[1..] {
        let s = a.stats("wall").context("no wall samples")?;
        writeln!(
            out,
            "{} against {}: {}",
            a.name,
            arms[0].name,
            verdict(base, s, b.floor)
        )?;
    }
    Ok(())
}

/// `cargo xtask arms <rev-a> <rev-b> [--bin <name>]`.
pub(crate) fn arms(args: &[String], root: &Path, default_bin: &str) -> Result<ExitCode> {
    let revs: Vec<&String> = args
        .iter()
        .skip(1)
        .take_while(|a| !a.starts_with("--"))
        .collect();
    ensure!(
        revs.len() == 2,
        "usage: cargo xtask arms <rev-a> <rev-b> [--bin <name>]"
    );
    let bin = args
        .iter()
        .skip_while(|a| *a != "--bin")
        .nth(1)
        .map_or(default_bin, String::as_str);
    let base = root.join("target/arms");
    let src = base.join("src");
    let mut built = Vec::new();
    for rev in &revs {
        if src.exists() {
            fs::remove_dir_all(&src)?;
        }
        fs::create_dir_all(&src)?;
        let tar = Command::new("git")
            .args(["archive", "--format=tar", rev])
            .current_dir(root)
            .output()
            .context("git archive")?;
        ensure!(
            tar.status.success(),
            "git archive {rev} failed: {}",
            String::from_utf8_lossy(&tar.stderr)
        );
        let mut x = Command::new("tar")
            // -m: extracted files get the current time, not the commit's, so cargo
            // sees every file as newer than the last arm's build and rebuilds it;
            // with the commit's times an older revision could look up to date.
            .args(["-x", "-m", "-C"])
            .arg(&src)
            .stdin(Stdio::piped())
            .spawn()?;
        x.stdin
            .take()
            .context("tar stdin")?
            .write_all(&tar.stdout)?;
        ensure!(x.wait()?.success(), "unpacking {rev} failed");
        let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["build", "--release", "--quiet", "-p", bin])
            .current_dir(&src)
            .env("CARGO_TARGET_DIR", base.join("target"))
            .status()?;
        ensure!(status.success(), "building {rev} failed");
        let out = base.join(rev.replace('/', "_"));
        fs::copy(base.join("target/release").join(bin), &out)?;
        built.push(out);
    }
    compare(&built[0], &built[1], &revs)
}

/// The text section of a binary: `__TEXT,__text` on macOS, `.text` on ELF.
fn text(bin: &Path) -> Result<Vec<u8>> {
    if cfg!(target_os = "macos") {
        let o = Command::new("otool")
            .args(["-t", "-X"])
            .arg(bin)
            .output()
            .context("otool")?;
        ensure!(o.status.success(), "otool failed");
        Ok(o.stdout)
    } else {
        let out = bin.with_extension("text");
        let o = Command::new("objcopy")
            .args(["-O", "binary", "--only-section=.text"])
            .arg(bin)
            .arg(&out)
            .output()
            .context("objcopy")?;
        ensure!(o.status.success(), "objcopy failed");
        Ok(fs::read(out)?)
    }
}

fn compare(a: &Path, b: &Path, revs: &[&String]) -> Result<ExitCode> {
    let (ba, bb) = (fs::read(a)?, fs::read(b)?);
    if ba == bb {
        println!(
            "{} and {}: byte-identical binaries -- neutral, proven; no timing needed",
            revs[0], revs[1]
        );
        return Ok(ExitCode::SUCCESS);
    }
    let (ta, tb) = (text(a)?, text(b)?);
    if ta == tb {
        println!(
            "{} and {}: identical machine code (text section), binaries differ elsewhere -- neutral for code; time it only if data layout changed",
            revs[0], revs[1]
        );
    } else {
        println!(
            "{} and {}: machine code differs -- time it: cargo xtask bench --arm a={} --arm b={} --aa",
            revs[0],
            revs[1],
            a.display(),
            b.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_ranges_are_no_result_and_disjoint_ones_are() {
        let base = Stats {
            median: 10.0,
            min: 9.0,
            max: 11.0,
        };
        assert!(
            verdict(
                base,
                Stats {
                    median: 10.5,
                    min: 10.0,
                    max: 12.0
                },
                1.0
            )
            .starts_with("overlap")
        );
        assert!(
            verdict(
                base,
                Stats {
                    median: 30.0,
                    min: 29.0,
                    max: 31.0
                },
                1.0
            )
            .starts_with("disjoint: slower")
        );
        assert!(
            verdict(
                base,
                Stats {
                    median: 11.6,
                    min: 11.5,
                    max: 11.7
                },
                2.0
            )
            .contains("under the 2 ms floor")
        );
    }

    #[test]
    fn the_median_of_an_even_count_is_the_middle_pair() {
        let s = Stats::of(&[4.0, 1.0, 3.0, 2.0]);
        assert_eq!((s.median, s.min, s.max), (2.5, 1.0, 4.0));
    }
}
