//! `cargo xtask tests`: tests that can fail.
//!
//! * **Never skip.** A test that returns early after probing its environment
//!   (`if !cfg!(..) { return }`, a missing tool) or prints that it skipped
//!   reads as a pass while testing nothing. A test that cannot run here is
//!   `#[cfg]`-gated whole and runs on the CI job that can (pillar T).
//! * **Exact floors.** `assert!(n > 40)` over 54 fixtures lets 13 vanish
//!   unnoticed; a control states the count: `assert_eq!(n, 54)`. A bound
//!   against 0 or 1 ("not empty") is a property, not a count, and is allowed.
//! * **Every `#[ignore]` is listed** in `ignores.toml` with its reason, and
//!   every listed one still exists: an ignore nobody counts is a test nobody
//!   runs.
//!
//! Scans every `#[test]` function in each member's `src/` and `tests/`.
//! Tests inside macros (`proptest!`) are not parsed; their bodies are held
//! by the properties themselves.

use std::collections::BTreeSet;
use std::fs;

use anyhow::{Context, Result};
use quote::ToTokens;
use serde::Deserialize;
use syn::visit::{self, Visit};
use syn::{BinOp, Expr, ExprIf, ItemFn, Lit, Macro};

use crate::report::Report;
use crate::workspace::{Workspace, files_under, read_toml};

const GATE: &str = "tests";

#[derive(Debug, Default, Deserialize)]
struct Ignores {
    #[serde(default)]
    ignore: Vec<Ignored>,
}

#[derive(Debug, Deserialize)]
struct Ignored {
    file: String,
    test: String,
    reason: String,
}

/// A problem in one test function.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Problem {
    /// Returns early after a `cfg!`/environment probe.
    Skips,
    /// Prints that it skipped.
    SaysSkipped,
    /// A lower bound against a literal where the count should be stated.
    InexactFloor,
    /// Carries `#[ignore]`.
    Ignored,
}

fn is_test(f: &ItemFn) -> bool {
    f.attrs.iter().any(|a| a.path().is_ident("test"))
}

fn is_ignored(f: &ItemFn) -> bool {
    f.attrs.iter().any(|a| a.path().is_ident("ignore"))
}

/// Whether an expression probes the environment: `cfg!(..)`, `env::var`,
/// `Command::new`, `which`.
fn probes(e: &Expr) -> bool {
    let text = e.to_token_stream().to_string().replace(' ', "");
    ["cfg!(", "env::var", "Command::new", "which("]
        .iter()
        .any(|p| text.contains(p))
}

fn returns(block: &syn::Block) -> bool {
    struct R(bool);
    impl<'a> Visit<'a> for R {
        fn visit_expr_return(&mut self, _: &'a syn::ExprReturn) {
            self.0 = true;
        }
    }
    let mut r = R(false);
    r.visit_block(block);
    r.0
}

struct Body(Vec<Problem>);

impl<'a> Visit<'a> for Body {
    fn visit_expr_if(&mut self, i: &'a ExprIf) {
        if probes(&i.cond) && returns(&i.then_branch) {
            self.0.push(Problem::Skips);
        }
        visit::visit_expr_if(self, i);
    }

    fn visit_macro(&mut self, m: &'a Macro) {
        let name = m
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        let tokens = m.tokens.to_string().to_lowercase();
        if matches!(name.as_str(), "println" | "eprintln" | "print" | "eprint")
            && tokens.contains("skip")
        {
            self.0.push(Problem::SaysSkipped);
        }
        if matches!(name.as_str(), "assert" | "debug_assert") {
            let first = m
                .parse_body_with(
                    syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated,
                )
                .ok()
                .and_then(|p| p.into_iter().next());
            if let Some(Expr::Binary(b)) = first {
                // A count floor: a literal of 2 or more. `n > 0` and `n >= 1`
                // say "not empty", a property rather than a control; a
                // control that says `checked > 40` over 54 fixtures lets 13
                // vanish, and is the case this catches.
                let lit = |e: &Expr| matches!(e, Expr::Lit(l) if matches!(&l.lit, Lit::Int(i) if i.base10_parse::<u128>().is_ok_and(|v| v >= 2)));
                let lower = matches!(b.op, BinOp::Gt(_) | BinOp::Ge(_)) && lit(&b.right)
                    || matches!(b.op, BinOp::Lt(_) | BinOp::Le(_)) && lit(&b.left);
                if lower {
                    self.0.push(Problem::InexactFloor);
                }
            }
        }
    }
}

/// How many test functions one file holds, and their problems by name.
pub(crate) fn scan(src: &str) -> syn::Result<(usize, Vec<(String, Problem)>)> {
    struct Tests(usize, Vec<(String, Problem)>);
    impl<'a> Visit<'a> for Tests {
        fn visit_item_fn(&mut self, f: &'a ItemFn) {
            if is_test(f) {
                self.0 += 1;
                let name = f.sig.ident.to_string();
                if is_ignored(f) {
                    self.1.push((name.clone(), Problem::Ignored));
                }
                let mut b = Body(Vec::new());
                b.visit_block(&f.block);
                self.1.extend(b.0.into_iter().map(|p| (name.clone(), p)));
            }
            visit::visit_item_fn(self, f);
        }
    }
    let mut t = Tests(0, Vec::new());
    t.visit_file(&syn::parse_file(src)?);
    Ok((t.0, t.1))
}

pub(crate) fn run(ws: &Workspace, r: &mut Report) -> Result<()> {
    control()?;
    let path = ws.root.join("ignores.toml");
    let listed: Ignores = if path.exists() {
        read_toml(&path)?
    } else {
        Ignores::default()
    };
    let mut seen = BTreeSet::new();
    let mut tests = 0;
    for m in &ws.members {
        for dir in ["src", "tests"] {
            for file in files_under(&m.dir().join(dir), &["rs"]).unwrap_or_default() {
                let rel = ws.rel(&file).to_string();
                let src = fs::read_to_string(&file)?;
                let (n, found) = scan(&src).with_context(|| format!("parsing {rel}"))?;
                tests += n;
                for (name, problem) in found {
                    report(r, &listed, &mut seen, &rel, &name, &problem);
                }
            }
        }
    }
    for i in &listed.ignore {
        if !seen.contains(&(i.file.clone(), i.test.clone())) {
            r.fail(
                GATE,
                "ignores.toml",
                format!(
                    "lists `{}` in {}, which is no longer an ignored test",
                    i.test, i.file
                ),
            );
        }
        if i.reason.trim().is_empty() {
            r.fail(GATE, "ignores.toml", format!("`{}` has no reason", i.test));
        }
    }
    r.checked(
        GATE,
        format!("{tests} #[test] functions in {} crates", ws.members.len()),
    );
    Ok(())
}

fn report(
    r: &mut Report,
    listed: &Ignores,
    seen: &mut BTreeSet<(String, String)>,
    rel: &str,
    name: &str,
    p: &Problem,
) {
    let place = format!("{rel}: {name}");
    match p {
        Problem::Skips => r.fail(GATE, place, "returns early after probing its environment: it would pass having tested nothing -- #[cfg]-gate the test whole and run it on the CI job that can"),
        Problem::SaysSkipped => r.fail(GATE, place, "prints that it skipped: a check that skips itself reads as a pass"),
        Problem::InexactFloor => r.fail(GATE, place, "asserts a lower bound against a literal: state the count with assert_eq!"),
        Problem::Ignored => {
            seen.insert((rel.to_string(), name.to_string()));
            if !listed.ignore.iter().any(|i| i.file == rel && i.test == name) {
                r.fail(GATE, place, "is #[ignore]d but not listed in ignores.toml with its reason");
            }
        }
    }
}

/// The positive control: each problem must be seen in a fixture built to
/// have exactly one of each.
fn control() -> Result<()> {
    let fixture = r#"
        #[test] fn skips() { if !cfg!(target_os = "macos") { return; } }
        #[test] fn says() { eprintln!("skipped: no device"); }
        #[test] fn floor() { let n = 5; assert!(n > 3); }
        #[test] #[ignore] fn ignored() {}
        #[test] fn fine() { let n = 54; assert_eq!(n, 54); assert!(n < limit()); }
    "#;
    let got: Vec<Problem> = scan(fixture)?.1.into_iter().map(|(_, p)| p).collect();
    let want = [
        Problem::Skips,
        Problem::SaysSkipped,
        Problem::InexactFloor,
        Problem::Ignored,
    ];
    anyhow::ensure!(
        got.len() == want.len() && want.iter().all(|w| got.contains(w)),
        "positive control failed: the test scanner found {got:?}, expected one each of {want:?}"
    );
    Ok(())
}
