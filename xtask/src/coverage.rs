//! `cargo xtask coverage`: every public function of domain and core is
//! called by some test.
//!
//! Not line coverage -- that reads high while saying little, and mutation
//! testing is the strength gate (`test:mutants-diff`). This asks the cheaper
//! question first: is there any test at all that reaches each function a
//! host can call? Calls are resolved by name from the syntax of test code
//! (`#[cfg(test)]` modules and `tests/`), including the token streams of
//! test macros such as `proptest!`, where a body is not parsed as Rust.

use std::collections::BTreeSet;
use std::fs;

use anyhow::{Context, Result};
use proc_macro2::{TokenStream, TokenTree};
use syn::visit::{self, Visit};
use syn::{Attribute, ImplItemFn, ItemFn, ItemImpl, ItemMod, Visibility};

use crate::report::Report;
use crate::workspace::{Workspace, files_under};

const GATE: &str = "coverage";

fn is_test_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("test")
            || (a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|i| i == "test"))
    })
}

/// Public functions and inherent methods outside test code.
#[derive(Default)]
struct Public {
    names: Vec<(String, usize)>,
    in_trait_impl: bool,
}

impl<'a> Visit<'a> for Public {
    fn visit_item_mod(&mut self, m: &'a ItemMod) {
        if !is_test_attr(&m.attrs) {
            visit::visit_item_mod(self, m);
        }
    }
    fn visit_item_fn(&mut self, f: &'a ItemFn) {
        if matches!(f.vis, Visibility::Public(_)) && !is_test_attr(&f.attrs) {
            self.names
                .push((f.sig.ident.to_string(), f.sig.ident.span().start().line));
        }
    }
    fn visit_item_impl(&mut self, i: &'a ItemImpl) {
        let outer = self.in_trait_impl;
        self.in_trait_impl = i.trait_.is_some();
        visit::visit_item_impl(self, i);
        self.in_trait_impl = outer;
    }
    fn visit_impl_item_fn(&mut self, f: &'a ImplItemFn) {
        if !self.in_trait_impl && matches!(f.vis, Visibility::Public(_)) {
            self.names
                .push((f.sig.ident.to_string(), f.sig.ident.span().start().line));
        }
    }
}

/// Every name called in test code: `f(..)`, `T::f(..)`, `x.f(..)`, and
/// `name(` inside macro token streams.
#[derive(Default)]
struct Called {
    names: BTreeSet<String>,
    depth: usize,
}

fn tokens_calls(ts: &TokenStream, out: &mut BTreeSet<String>) {
    let mut prev: Option<String> = None;
    for tt in ts.clone() {
        match tt {
            TokenTree::Ident(i) => prev = Some(i.to_string()),
            TokenTree::Group(g) => {
                if g.delimiter() == proc_macro2::Delimiter::Parenthesis {
                    if let Some(p) = prev.take() {
                        out.insert(p);
                    }
                }
                tokens_calls(&g.stream(), out);
                prev = None;
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => {
                if !matches!(&tt, TokenTree::Punct(p) if p.as_char() == ':') {
                    prev = None;
                }
            }
        }
    }
}

impl<'a> Visit<'a> for Called {
    fn visit_item_mod(&mut self, m: &'a ItemMod) {
        let test = is_test_attr(&m.attrs);
        self.depth += usize::from(test);
        visit::visit_item_mod(self, m);
        self.depth -= usize::from(test);
    }
    fn visit_item_fn(&mut self, f: &'a ItemFn) {
        let test = is_test_attr(&f.attrs);
        self.depth += usize::from(test);
        visit::visit_item_fn(self, f);
        self.depth -= usize::from(test);
    }
    fn visit_expr_call(&mut self, c: &'a syn::ExprCall) {
        if self.depth > 0 {
            if let syn::Expr::Path(p) = &*c.func {
                if let Some(s) = p.path.segments.last() {
                    self.names.insert(s.ident.to_string());
                }
            }
        }
        visit::visit_expr_call(self, c);
    }
    fn visit_expr_method_call(&mut self, m: &'a syn::ExprMethodCall) {
        if self.depth > 0 {
            self.names.insert(m.method.to_string());
        }
        visit::visit_expr_method_call(self, m);
    }
    fn visit_macro(&mut self, m: &'a syn::Macro) {
        if self.depth > 0 {
            tokens_calls(&m.tokens, &mut self.names);
        }
    }
}

/// Test code: whole files under `tests/`, `#[cfg(test)]` code elsewhere.
fn called_in(src: &str, whole_file_is_test: bool) -> syn::Result<BTreeSet<String>> {
    let mut c = Called {
        depth: usize::from(whole_file_is_test),
        ..Called::default()
    };
    c.visit_file(&syn::parse_file(src)?);
    Ok(c.names)
}

pub(crate) fn run(ws: &Workspace, r: &mut Report) -> Result<()> {
    control()?;
    let mut called = BTreeSet::new();
    for m in &ws.members {
        for (dir, whole) in [("src", false), ("tests", true)] {
            for f in files_under(&m.dir().join(dir), &["rs"]).unwrap_or_default() {
                let src = fs::read_to_string(&f)?;
                called.extend(
                    called_in(&src, whole).with_context(|| format!("parsing {}", f.display()))?,
                );
            }
        }
    }
    let mut public = 0;
    for m in ws.members.iter().filter(|m| {
        ws.role(&m.name)
            .is_some_and(crate::workspace::Role::is_pure)
    }) {
        for f in files_under(&m.dir().join("src"), &["rs"])? {
            let mut p = Public::default();
            p.visit_file(&syn::parse_file(&fs::read_to_string(&f)?)?);
            for (name, line) in p.names {
                public += 1;
                if !called.contains(&name) {
                    r.fail(
                        GATE,
                        format!("{}:{line}", ws.rel(&f)),
                        format!("`{name}` is public and no test calls it"),
                    );
                }
            }
        }
    }
    r.checked(
        GATE,
        format!("{public} public functions of domain and core"),
    );
    Ok(())
}

/// The positive control: a call inside a macro, a method call and a path
/// call must each be seen, and a function nothing calls must not be.
fn control() -> Result<()> {
    let src = r"
        #[test] fn t() { a(); x.b(); T::c(); proptest! { fn p() { d(1) } } }
        fn not_a_test() { e(); }
    ";
    let got = called_in(src, false)?;
    let want: BTreeSet<String> = ["a", "b", "c", "d", "p"]
        .iter()
        .map(ToString::to_string)
        .collect();
    anyhow::ensure!(
        want.iter().all(|w| got.contains(w)) && !got.contains("e"),
        "positive control failed: the call scanner found {got:?}"
    );
    Ok(())
}
