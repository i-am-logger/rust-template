<div align="center">

# my-project

[![crates.io](https://img.shields.io/crates/v/my-project?logo=rust&logoColor=white)](https://crates.io/crates/my-project)
[![Downloads](https://img.shields.io/crates/d/my-project?logo=rust&logoColor=white)](https://crates.io/crates/my-project)
[![MSRV](https://img.shields.io/crates/msrv/my-project?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![CI](https://img.shields.io/github/actions/workflow/status/i-am-logger/my-project/ci.yml?branch=master&label=CI&logo=githubactions&logoColor=white)](https://github.com/i-am-logger/my-project/actions/workflows/ci.yml)
[![docs.rs](https://img.shields.io/docsrs/my-project?logo=docsdotrs&logoColor=white)](https://docs.rs/my-project)
[![codecov](https://codecov.io/gh/i-am-logger/my-project/branch/master/graph/badge.svg)](https://codecov.io/gh/i-am-logger/my-project)

[![Nix](https://img.shields.io/badge/Nix-2b2b2b?logo=nixos&logoColor=white)](https://nixos.org)
[![devenv](https://img.shields.io/badge/devenv-2b2b2b?logo=nixos&logoColor=white)](https://devenv.sh)
[![License: CC BY-NC-SA 4.0](https://img.shields.io/badge/CC%20BY--NC--SA%204.0-2b2b2b?logo=creativecommons&logoColor=white)](LICENSE)

</div>

> A Rust workspace built on the rust-code pillars: a domain type system, an
> event-driven core, enforced layering, zero-copy, `no_std` and no `unsafe` --
> each one a gate that fails the build, not a guideline.

## The pillars

| # | Pillar | The rule | Its gate |
|---|---|---|---|
| 1 | Type system | Every quantity is a type; no raw primitive in a domain or core public API outside a named seam; no `bool` parameter or field; illegal programs do not compile | `cargo xtask budgets`; trybuild cases with passing twins |
| 2 | Event-driven | The core takes events and queues effects for the host to poll: no callbacks, no clock, no I/O; effects do not depend on how input was split | `cargo xtask arch`; clippy bans; the `chunk_invariance_*` property |
| 3 | Layering | Every crate has a role (`layers.toml`) that says what it may depend on; features are additive | `cargo xtask layers`; `cargo hack --each-feature` |
| 4 | Zero-copy | Input is borrowed; no allocation per event; own, don't leak | clippy bans `Box::leak`/`mem::forget`; the allocation test |
| 5 | `no_std` | Domain and core build with no std at all | `cargo xtask arch`; `test:bare` |
| 6 | No `unsafe` | Forbidden workspace-wide; the only escape is a declared quarantine crate | the `forbid` lint; `cargo xtask arch` |

Every gate runs a positive control first -- a fixture it knows is wrong --
and fails if it does not find the fault, so a scanner that matches nothing
cannot read as a clean tree. `cargo xtask controls` (the last task of
`devenv test`) goes further: it plants one violation per gate in a scratch
copy and requires each gate to fail **for the planted reason**.

### Tests that can fail, and how to measure

| Gate | What it holds |
|---|---|
| `cargo xtask tests` | no test skips itself into a pass; controls state their count (`assert_eq!`, not `n > K`); every `#[ignore]` is listed in `ignores.toml` with its reason |
| `cargo xtask coverage` | every public function of domain and core is called by some test |
| `test:mutants-diff` | every mutant of a changed line in domain or core is caught (cargo-mutants); line coverage is not a gate |
| `test:unit` | the `qa` profile: release speed with overflow checks and `debug_assert!` on; proptest seed pinned |
| `test:deny` | licenses, bans and sources of every dependency |

| Instrument | What it does |
|---|---|
| `cargo xtask arms <rev> <rev>` | builds both revisions from `git archive` at one path and compares the machine code first: byte-identical is neutral, with no timing |
| `cargo xtask bench --arm a=<bin> --arm b=<bin> --aa` | timed launches, order rotated, first round dropped, medians with ranges, an A/A arm; overlapping ranges are no result |
| `MY_PROJECT_STARTUP=1` | the driver prints a mark per startup stage, which `bench` tabulates |

Manual or scheduled: `qa:mutants` (every mutant), `qa:props` (20 000 cases,
fresh seeds), `qa:advisories` (RustSec). Adopting the gates in an existing
workspace: `cargo xtask init` (every budget 0) or `cargo xtask init
--ratchet` (today's counts, which may only shrink).

The reasoning behind each rule, with its evidence, lives in the
[rust-code](https://github.com/i-am-logger/rust-code) plugin's skills, which
`.claude/settings.json` enables for Claude Code sessions in this repository
(after you trust the folder; each person installs it once with `claude plugin
install rust-code@rust-code --scope project`). Before the plugin is published,
use a local copy instead: `claude --plugin-dir ~/Code/rust-code`, or point the
marketplace at it with `"source": { "source": "directory", "path": "<absolute
path>" }`.

## Layout

| path | role | what |
|---|---|---|
| `crates/my-project-domain` | domain | the type system: quantities, typestate, the event vocabulary |
| `crates/my-project-core` | core | the machine: `handle(Event)`, effects polled |
| `crates/my-project` | bin | the driver: stdin, the clock, stdout |
| `xtask` | tool | the gates |

## Development

```bash
# Enter devenv shell
direnv allow

# Build / run the binary
dev-build
dev-run

# Every gate, exactly as CI runs it
dev-ci            # = devenv test
```

`dev-ci` is not an approximation of CI. `.github/workflows/ci.yml` runs
`devenv test`, which runs the `tasks."test:*"` set defined in `devenv.nix`.
Add a check by adding a task, never by adding a step to the workflow, and the
two cannot drift apart.

Formatting is `treefmt` across the whole tree (Rust, Nix, shell), wired
identically into `dev-fmt`, `tasks."test:fmt"`, and the pre-commit hook.

## Releases

Automated with [release-plz](https://release-plz.dev). Commits follow
[Conventional Commits](https://www.conventionalcommits.org/); release-plz opens
a `chore: release` PR that bumps the version and writes `CHANGELOG.md`, and
**merging that PR is what authorises the publish to crates.io.** Versions are
never edited by hand — release-plz reads its baseline from crates.io, so a
manual bump or publish desynchronises it.

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Funding

Solo-maintained, and sponsorship buys maintenance time rather than obligations.
See [FUNDING.md](FUNDING.md) — including how to arrange a commercial licence,
which the NonCommercial clause below makes a separate conversation.

## License

Creative Commons Attribution-NonCommercial-ShareAlike (CC BY-NC-SA) 4.0 International

See [LICENSE](LICENSE) for details.
