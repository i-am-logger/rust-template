# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0](https://github.com/i-am-logger/rust-template/releases/tag/v0.1.0) - 2026-08-09

### Chore

- sync template with pr4xis conventions ([#1](https://github.com/i-am-logger/rust-template/pull/1))

### Feat

- add dev-ci, RUSTFLAGS -D warnings, codecov in CI
- add codecov, concurrency cancel, multi-row badges
- add crates.io publish pipeline and release-please config

### Fix

- update devenv.lock, SKIP git-hooks in CI test
- skip CI on release-please merge commits, revert SKIP hack
- skip git hooks during devenv test (CI)
- nix badge before rust
