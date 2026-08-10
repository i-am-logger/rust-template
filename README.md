# my-project

[![crates.io](https://img.shields.io/crates/v/my-project?logo=rust&logoColor=white)](https://crates.io/crates/my-project)
[![Downloads](https://img.shields.io/crates/d/my-project?logo=rust&logoColor=white)](https://crates.io/crates/my-project)
[![MSRV](https://img.shields.io/crates/msrv/my-project?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![CI](https://img.shields.io/github/actions/workflow/status/i-am-logger/my-project/ci.yml?branch=master&label=CI&logo=githubactions&logoColor=white)](https://github.com/i-am-logger/my-project/actions/workflows/ci.yml)
[![docs.rs](https://img.shields.io/docsrs/my-project?logo=docsdotrs&logoColor=white)](https://docs.rs/my-project)
[![codecov](https://codecov.io/gh/i-am-logger/my-project/branch/master/graph/badge.svg)](https://codecov.io/gh/i-am-logger/my-project)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/i-am-logger/my-project/badge)](https://scorecard.dev/viewer/?uri=github.com/i-am-logger/my-project)

[![Nix](https://img.shields.io/badge/Nix-2b2b2b?logo=nixos&logoColor=white)](https://nixos.org)
[![devenv](https://img.shields.io/badge/devenv-2b2b2b?logo=nixos&logoColor=white)](https://devenv.sh)
[![License: CC BY-NC-SA 4.0](https://img.shields.io/badge/CC%20BY--NC--SA%204.0-2b2b2b?logo=creativecommons&logoColor=white)](LICENSE)

> A Rust CLI project.

## Development

```bash
# Enter devenv shell
direnv allow

# Build
dev-build

# Run
dev-run

# Test
dev-test

# Full CI pipeline locally
dev-ci
```

`dev-ci` is not an approximation of CI. `.github/workflows/ci.yml` runs
`devenv test`, which runs the `tasks."test:*"` set defined in `devenv.nix` —
the same suite `dev-ci` walks. Add a check by adding a task, never by adding a
step to the workflow, and the two cannot drift apart.

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

## License

Creative Commons Attribution-NonCommercial-ShareAlike (CC BY-NC-SA) 4.0 International

See [LICENSE](LICENSE) for details.
