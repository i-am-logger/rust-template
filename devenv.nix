{ pkgs
, config
, lib
, ...
}:
let
  # A virtual workspace has no [package]: the project's name and description
  # live in [workspace.metadata.project], its version in [workspace.package].
  cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
  packageName = cargoToml.workspace.metadata.project.name;
  packageVersion = cargoToml.workspace.package.version;
  packageDescription = cargoToml.workspace.metadata.project.description or "";
in
{
  # Set root explicitly for flake compatibility
  devenv.root = lib.mkDefault (builtins.toString ./.);

  dotenv.enable = true;
  imports = [
    ./nix/rust.nix
  ];

  # Additional packages for development
  packages = [
    pkgs.git
    pkgs.pkg-config
    # The gates' tools. Pinned by devenv.lock, so CI and every shell run the
    # same versions.
    pkgs.cargo-nextest
    pkgs.cargo-hack
    pkgs.cargo-deny
    pkgs.cargo-mutants
    pkgs.cargo-llvm-cov
  ];

  # Development scripts
  scripts.dev-test.exec = ''
    echo "Running tests..."
    devenv tasks run test:unit
  '';

  scripts.dev-fmt.exec = ''
    echo "Checking formatting..."
    treefmt --fail-on-change
  '';

  scripts.dev-lint.exec = ''
    echo "Running clippy..."
    cargo clippy --quiet --workspace --all-targets --all-features -- -D warnings
  '';

  scripts.dev-check.exec = ''
    echo "Checking compilation..."
    cargo check --quiet --workspace --all-targets
  '';

  # The same suite CI runs, by construction: `devenv test` walks the
  # tasks."test:*" set below.
  scripts.dev-ci.exec = ''
    echo "Running full CI pipeline locally..."
    devenv test
  '';

  scripts.dev-run.exec = ''
    echo "Running ${packageName}..."
    cargo run --release -p ${packageName}
  '';

  scripts.dev-build.exec = ''
    echo "Building ${packageName}..."
    cargo build --release -p ${packageName}
  '';

  # Environment variables
  env = {
    CARGO_TARGET_DIR = "./target";
  };

  # Development shell setup.
  #
  # The whole banner goes to stderr. `devenv shell -- <cmd>` runs this first, so
  # anything on stdout is prepended to that command's output - and CI runs
  # `devenv shell -- cargo metadata` as Swatinem/rust-cache's cmd-format probe,
  # where a banner in front of the JSON is unparseable. The action swallows that
  # error, keeps the step green, and caches nothing.
  enterShell = ''
    {
      clear
      ${pkgs.figlet}/bin/figlet "${packageName}"
      echo
      {
        ${pkgs.lib.optionalString (packageDescription != "") ''echo "• ${packageDescription}"''}
        echo -e "• \033[1mv${packageVersion}\033[0m"
        echo -e " \033[0;32m✓\033[0m Development environment ready"
      } | ${pkgs.boxes}/bin/boxes -d stone -a l -i none
      echo
      echo "Available scripts:"
      echo "  dev-ci        - Run the full gate suite (devenv test)"
      echo "  dev-test      - Run tests"
      echo "  dev-fmt       - Check formatting"
      echo "  dev-lint      - Run clippy"
      echo "  dev-check     - Check compilation"
      echo "  dev-run       - Run the application"
      echo "  dev-build     - Build the application"
      echo ""
    } >&2
  '';

  # https://devenv.sh/integrations/treefmt/
  #
  # One formatter for the whole tree, not just Rust — a Nix or shell file that
  # CI reformats is as much a diff-noise source as an unformatted .rs. `treefmt`
  # is the single entry point for `dev-fmt`, `tasks."test:fmt"`, and the
  # pre-commit hook, so all three can only ever agree.
  treefmt = {
    enable = true;
    config = {
      settings.global.excludes = [
        ".devenv.flake.nix"
        ".devenv/"
      ];

      programs = {
        # Nix
        nixpkgs-fmt.enable = true;
        deadnix = {
          enable = true;
          no-underscore = true;
        };
        statix.enable = true;

        # Rust — use the devenv toolchain so the formatter understands the
        # same edition the compiler does.
        rustfmt = {
          enable = true;
          package = config.languages.rust.toolchainPackage;
        };

        # Shell
        shellcheck.enable = true;
        shfmt.enable = true;
      };
    };
  };

  # https://devenv.sh/git-hooks/
  git-hooks.settings.rust.cargoManifestPath = "./Cargo.toml";

  # Use the same Rust toolchain for git-hooks as for development
  git-hooks.tools = {
    cargo = lib.mkForce config.languages.rust.toolchainPackage;
    clippy = lib.mkForce config.languages.rust.toolchainPackage;
    rustfmt = lib.mkForce config.languages.rust.toolchainPackage;
  };

  # treefmt only. A clippy hook would run with its own default flags - no
  # --all-targets, no --all-features - which is a different cargo fingerprint
  # from the test:clippy task, so it compiles the crate under clippy a second
  # time and cannot fail anything that task does not. Formatting is the part
  # worth catching before the commit rather than after.
  git-hooks.hooks = {
    treefmt.enable = true;
  };

  # https://devenv.sh/tasks/
  #
  # These tasks ARE the check suite. `enterTest` runs them, so `devenv test`
  # locally and `devenv test` in .github/workflows/ci.yml execute the same
  # thing by construction -- the workflow is a thin wrapper, and the two cannot
  # drift apart. Add a check by adding a task here, never a step to the workflow.
  #
  # The `after` edges are load-bearing. devenv runs tasks with no edge between
  # them concurrently, and every cargo task blocks on the one lock over
  # ./target. Serialised, they run cheapest first -- a formatting failure costs
  # no compilation -- and each reuses what the one before it compiled.
  #
  # The pillars each have a gate here (see README.md, "The pillars"):
  #   P1 type system   test:budgets (primitives, bool, pub newtype fields),
  #                    test:unit (trybuild compile_fail cases and their twins)
  #   P2 sans-IO       test:arch (no callbacks, chunk-invariance test present),
  #                    test:clippy (no clock/RNG in domain and core)
  #   P3 layering      test:layers (cargo metadata against layers.toml),
  #                    test:features (every feature builds alone)
  #   P4 zero-copy     test:clippy (no leaks), test:unit (no allocation per event)
  #   P5 no_std        test:arch (the attribute), test:bare (bare-metal builds)
  #   P6 no unsafe     the `forbid` lint, test:arch (workspace lints everywhere),
  #                    test:budgets (unsafe counted)
  tasks = {
    "test:fmt" = {
      exec = "treefmt --fail-on-change";
    };

    "test:sources" = {
      exec = "cargo xtask sources";
      after = [ "test:fmt" ];
    };

    "test:clippy" = {
      exec = "cargo clippy --quiet --workspace --all-targets --all-features -- -D warnings";
      after = [ "test:sources" ];
    };

    "test:check" = {
      exec = "cargo check --quiet --workspace --all-targets";
      after = [ "test:clippy" ];
    };

    "test:arch" = {
      exec = "cargo xtask arch";
      after = [ "test:check" ];
    };

    "test:layers" = {
      exec = "cargo xtask layers";
      after = [ "test:arch" ];
    };

    "test:budgets" = {
      exec = "cargo xtask budgets";
      after = [ "test:layers" ];
    };

    "test:docs" = {
      exec = "cargo xtask docs";
      after = [ "test:budgets" ];
    };

    # Under the `qa` profile: release speed with overflow checks and
    # debug_assert! on. The proptest seed and case count are pinned so the
    # gate is deterministic; `qa:props` runs fresh seeds at 20 000 cases.
    # Both halves always run (--no-fail-fast), and either failing fails the task.
    # All features: they are additive (test:features), so this is the superset,
    # and code behind `alloc` -- where an allocation per event could hide -- is
    # exercised rather than compiled out.
    "test:unit" = {
      exec = ''
        export PROPTEST_CASES=256 PROPTEST_RNG_SEED=1
        cargo nextest run --workspace --all-features --cargo-profile qa --no-fail-fast --cargo-quiet; a=$?
        cargo test --workspace --all-features --doc --profile qa --quiet; b=$?
        [ "$a" -eq 0 ] && [ "$b" -eq 0 ]
      '';
      after = [ "test:docs" ];
    };

    "test:doc" = {
      exec = "RUSTDOCFLAGS='-D warnings' cargo doc --quiet --workspace --no-deps";
      after = [ "test:unit" ];
    };

    # Pillar 5: domain and core build for targets that have no std at all, with
    # every default feature off. A `use std` anywhere fails here at link.
    "test:bare" = {
      # The crate list comes from layers.toml, never from here: a second
      # domain crate is covered the moment it is given its role.
      exec = ''
        crates=$(cargo xtask crates --role domain,core) || exit 1
        for t in x86_64-unknown-none thumbv7em-none-eabihf; do
          # shellcheck disable=SC2086 # one word per -p argument
          cargo build --quiet --no-default-features --target "$t" $crates || exit 1
        done
      '';
      after = [ "test:doc" ];
    };

    # Pillar 3: features are additive. Every feature of every crate builds on
    # its own, so no feature can depend on another being on.
    "test:features" = {
      exec = "cargo hack check --quiet --workspace --each-feature --no-dev-deps";
      after = [ "test:bare" ];
    };

    # The gates' own gate: one violation planted per gate in a scratch copy
    # (target/controls), and each gate must fail for the planted reason. Last,
    # because it rebuilds per case; it is what makes a green run above mean
    # something rather than "nothing was looked at".
    "test:controls" = {
      exec = "cargo xtask controls";
      after = [ "test:features" ];
    };
  };

  # https://devenv.sh/tests/
  # Every gate must be named here: a task left out is a task CI never runs.
  enterTest = lib.mkForce "devenv tasks run test:fmt test:sources test:clippy test:check test:arch test:layers test:budgets test:docs test:unit test:doc test:bare test:features test:controls";
}
