//! Counting constructs in Rust source, by parsing it with `syn` rather than by
//! matching text, so a count is a fact about the code and not about its
//! formatting. Test code (`#[cfg(test)]`, `#[test]`) is not counted: the rules
//! are about what ships.

use std::collections::BTreeMap;

use proc_macro2::Span;
use syn::visit::{self, Visit};
use syn::{
    Attribute, Fields, FnArg, GenericArgument, ItemEnum, ItemFn, ItemImpl, ItemMod, ItemStruct,
    ItemTrait, PathArguments, ReturnType, Signature, Type, TypeParamBound, Visibility,
};

/// What the scanner counts.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Kind {
    /// A raw integer, float or `char` in a public signature or field, outside
    /// a named seam (pillar 1).
    PrimitiveApi,
    /// A public function whose name is a declared seam (`get`, `of`, ...) and
    /// whose signature carries a primitive: the counted ways in and out.
    Seam,
    /// A tuple struct with a public field: a newtype that leaks its inside.
    PubNewtypeField,
    /// A `bool` parameter or field: a two-variant enum says what it means.
    BoolParamOrField,
    /// An `as` cast.
    AsCast,
    /// An `#[expect(...)]` suppression.
    ExpectAttr,
    /// An `unsafe` block, function, impl or trait (pillar 6).
    UnsafeCode,
    /// A callback in a public signature: `fn(..)`, or a trait object, `impl
    /// Trait` or generic bounded by a behaviour trait -- `Fn*` or a sink like
    /// `&mut dyn Sink` (pillar 2: effects are polled, not called back).
    CallbackParam,
}

impl Kind {
    pub(crate) const ALL: [Self; 8] = [
        Self::PrimitiveApi,
        Self::Seam,
        Self::PubNewtypeField,
        Self::BoolParamOrField,
        Self::AsCast,
        Self::ExpectAttr,
        Self::UnsafeCode,
        Self::CallbackParam,
    ];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::PrimitiveApi => "primitive_api",
            Self::Seam => "seam",
            Self::PubNewtypeField => "pub_newtype_field",
            Self::BoolParamOrField => "bool_param_or_field",
            Self::AsCast => "as_cast",
            Self::ExpectAttr => "expect_attr",
            Self::UnsafeCode => "unsafe_code",
            Self::CallbackParam => "callback_param",
        }
    }
}

/// One counted construct and where it is.
#[derive(Clone, Debug)]
pub(crate) struct Hit {
    pub(crate) kind: Kind,
    pub(crate) line: usize,
    pub(crate) what: String,
}

/// Counts per kind.
pub(crate) type Counts = BTreeMap<Kind, usize>;

pub(crate) fn count(hits: &[Hit]) -> Counts {
    let mut c: Counts = Kind::ALL.iter().map(|&k| (k, 0)).collect();
    for h in hits {
        *c.entry(h.kind).or_default() += 1;
    }
    c
}

/// Scans one file. `seams` names the functions allowed to carry primitives.
pub(crate) fn scan(src: &str, seams: &[String]) -> syn::Result<Vec<Hit>> {
    let file = syn::parse_file(src)?;
    let mut v = Scanner {
        seams,
        hits: Vec::new(),
        in_trait_impl: false,
    };
    v.visit_file(&file);
    Ok(v.hits)
}

struct Scanner<'s> {
    seams: &'s [String],
    hits: Vec<Hit>,
    in_trait_impl: bool,
}

const INTS: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32",
    "f64", "char",
];
/// Traits a public core parameter may be bounded by: they describe data, not
/// behaviour the core would call back into. Any other trait object, `impl
/// Trait` or bounded generic is a sink -- a callback by another name.
const DATA_TRAITS: &[&str] = &[
    "Iterator",
    "IntoIterator",
    "AsRef",
    "Borrow",
    "Into",
    "Copy",
    "Clone",
    "Debug",
    "Sized",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Hash",
];

fn is_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("test")
            || (a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|i| i == "test"))
    })
}

fn is_pub(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

fn line(span: Span) -> usize {
    span.start().line
}

/// Whether a path's generic arguments mention a raw primitive: `Option<u8>`,
/// `Iterator<Item = u8>`, `Fn(u8) -> u32`.
fn args_have_primitive(args: &PathArguments) -> bool {
    match args {
        PathArguments::AngleBracketed(a) => a.args.iter().any(|g| match g {
            GenericArgument::Type(t) => has_primitive(t),
            GenericArgument::AssocType(at) => has_primitive(&at.ty),
            _ => false,
        }),
        PathArguments::Parenthesized(p) => {
            p.inputs.iter().any(has_primitive)
                || matches!(&p.output, ReturnType::Type(_, t) if has_primitive(t))
        }
        PathArguments::None => false,
    }
}

fn bounds_have_primitive<'a>(mut bounds: impl Iterator<Item = &'a TypeParamBound>) -> bool {
    bounds.any(|b| matches!(b, TypeParamBound::Trait(t) if t.path.segments.iter().any(|s| args_have_primitive(&s.arguments))))
}

/// Whether a type mentions a raw primitive anywhere inside it, including in
/// trait bounds (`impl Iterator<Item = u8>`) and function types.
fn has_primitive(ty: &Type) -> bool {
    match ty {
        Type::Path(p) => p
            .path
            .segments
            .iter()
            .any(|s| INTS.iter().any(|i| s.ident == i) || args_have_primitive(&s.arguments)),
        Type::ImplTrait(t) => bounds_have_primitive(t.bounds.iter()),
        Type::TraitObject(t) => bounds_have_primitive(t.bounds.iter()),
        Type::BareFn(f) => {
            f.inputs.iter().any(|a| has_primitive(&a.ty))
                || matches!(&f.output, ReturnType::Type(_, t) if has_primitive(t))
        }
        Type::Reference(r) => has_primitive(&r.elem),
        Type::Slice(s) => has_primitive(&s.elem),
        Type::Array(a) => has_primitive(&a.elem),
        Type::Ptr(p) => has_primitive(&p.elem),
        Type::Tuple(t) => t.elems.iter().any(has_primitive),
        Type::Paren(p) => has_primitive(&p.elem),
        Type::Group(g) => has_primitive(&g.elem),
        _ => false,
    }
}

fn is_bool(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.path.is_ident("bool"))
}

fn bound_is_callback(b: &TypeParamBound) -> bool {
    matches!(b, TypeParamBound::Trait(t)
        if t.path.segments.last().is_some_and(|s| !DATA_TRAITS.iter().any(|c| s.ident == c)))
}

/// Whether a type is or contains a callback.
fn is_callback(ty: &Type) -> bool {
    match ty {
        Type::BareFn(_) => true,
        Type::ImplTrait(t) => t.bounds.iter().any(bound_is_callback),
        Type::TraitObject(t) => t.bounds.iter().any(bound_is_callback),
        Type::Reference(r) => is_callback(&r.elem),
        Type::Paren(p) => is_callback(&p.elem),
        Type::Group(g) => is_callback(&g.elem),
        Type::Path(p) => p.path.segments.iter().any(|s| match &s.arguments {
            PathArguments::AngleBracketed(a) => a
                .args
                .iter()
                .any(|g| matches!(g, GenericArgument::Type(t) if is_callback(t))),
            _ => false,
        }),
        _ => false,
    }
}

impl Scanner<'_> {
    fn hit(&mut self, kind: Kind, span: Span, what: impl Into<String>) {
        self.hits.push(Hit {
            kind,
            line: line(span),
            what: what.into(),
        });
    }

    /// Every function: bool params. Public ones also: primitives, callbacks.
    fn signature(&mut self, sig: &Signature, public: bool) {
        let name = sig.ident.to_string();
        if sig.unsafety.is_some() {
            self.hit(
                Kind::UnsafeCode,
                sig.ident.span(),
                format!("unsafe fn {name}"),
            );
        }
        let mut primitive = false;
        for arg in &sig.inputs {
            if let FnArg::Typed(pt) = arg {
                if is_bool(&pt.ty) {
                    self.hit(
                        Kind::BoolParamOrField,
                        sig.ident.span(),
                        format!("fn {name}: bool parameter"),
                    );
                }
                primitive |= has_primitive(&pt.ty);
                if public && is_callback(&pt.ty) {
                    self.hit(
                        Kind::CallbackParam,
                        sig.ident.span(),
                        format!("fn {name}: callback parameter"),
                    );
                }
            }
        }
        if public {
            let generic_callback = sig.generics.type_params().any(|p| p.bounds.iter().any(bound_is_callback))
                || sig.generics.where_clause.as_ref().is_some_and(|w| {
                    w.predicates.iter().any(|p| matches!(p, syn::WherePredicate::Type(t) if t.bounds.iter().any(bound_is_callback)))
                });
            if generic_callback {
                self.hit(
                    Kind::CallbackParam,
                    sig.ident.span(),
                    format!("fn {name}: generic bounded by Fn*"),
                );
            }
            if let ReturnType::Type(_, ty) = &sig.output {
                primitive |= has_primitive(ty);
            }
            if primitive {
                let kind = if self.seams.contains(&name) {
                    Kind::Seam
                } else {
                    Kind::PrimitiveApi
                };
                self.hit(kind, sig.ident.span(), format!("fn {name}"));
            }
        }
    }

    fn fields(&mut self, fields: &Fields, owner: &str, public_owner: bool) {
        for f in fields {
            let span = f
                .ident
                .as_ref()
                .map_or_else(Span::call_site, syn::Ident::span);
            if is_bool(&f.ty) {
                self.hit(Kind::BoolParamOrField, span, format!("{owner}: bool field"));
            }
            if public_owner && is_pub(&f.vis) && has_primitive(&f.ty) && f.ident.is_some() {
                self.hit(
                    Kind::PrimitiveApi,
                    span,
                    format!("{owner}: public primitive field"),
                );
            }
        }
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_attribute(&mut self, a: &'ast Attribute) {
        if a.path().is_ident("expect") {
            self.hit(Kind::ExpectAttr, a.pound_token.span, "#[expect(..)]");
        }
    }

    fn visit_item_mod(&mut self, m: &'ast ItemMod) {
        if !is_test(&m.attrs) {
            visit::visit_item_mod(self, m);
        }
    }

    fn visit_item_fn(&mut self, f: &'ast ItemFn) {
        if is_test(&f.attrs) {
            return;
        }
        self.signature(&f.sig, is_pub(&f.vis));
        visit::visit_item_fn(self, f);
    }

    fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
        if is_test(&i.attrs) {
            return;
        }
        if i.unsafety.is_some() {
            self.hit(Kind::UnsafeCode, i.impl_token.span, "unsafe impl");
        }
        if let Some((_, path, _)) = &i.trait_ {
            let generic_primitive = path.segments.iter().any(|seg| match &seg.arguments {
                PathArguments::AngleBracketed(a) => a
                    .args
                    .iter()
                    .any(|g| matches!(g, GenericArgument::Type(t) if has_primitive(t))),
                _ => false,
            });
            let trait_name = path
                .segments
                .last()
                .map_or_else(String::new, |s| s.ident.to_string());
            if generic_primitive {
                self.hit(
                    Kind::PrimitiveApi,
                    i.impl_token.span,
                    format!(
                        "impl {trait_name}<primitive>: a primitive crosses the API through a trait"
                    ),
                );
            }
            for item in &i.items {
                if let syn::ImplItem::Type(t) = item {
                    if has_primitive(&t.ty) {
                        self.hit(
                            Kind::PrimitiveApi,
                            t.ident.span(),
                            format!("impl {trait_name}: type {} = primitive", t.ident),
                        );
                    }
                }
            }
        }
        let outer = self.in_trait_impl;
        self.in_trait_impl = i.trait_.is_some();
        visit::visit_item_impl(self, i);
        self.in_trait_impl = outer;
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        if is_test(&f.attrs) {
            return;
        }
        // A trait impl's methods have no visibility of their own; the trait
        // decides the signature, so they are not this crate's API choices.
        let public = !self.in_trait_impl && is_pub(&f.vis);
        self.signature(&f.sig, public);
        visit::visit_impl_item_fn(self, f);
    }

    fn visit_item_trait(&mut self, t: &'ast ItemTrait) {
        if t.unsafety.is_some() {
            self.hit(Kind::UnsafeCode, t.trait_token.span, "unsafe trait");
        }
        let public = is_pub(&t.vis);
        for item in &t.items {
            if let syn::TraitItem::Fn(f) = item {
                self.signature(&f.sig, public);
            }
        }
        visit::visit_item_trait(self, t);
    }

    fn visit_item_struct(&mut self, s: &'ast ItemStruct) {
        if is_test(&s.attrs) {
            return;
        }
        let name = s.ident.to_string();
        if let Fields::Unnamed(u) = &s.fields {
            if u.unnamed.iter().any(|f| is_pub(&f.vis)) {
                self.hit(
                    Kind::PubNewtypeField,
                    s.ident.span(),
                    format!("struct {name}: public tuple field"),
                );
            }
        }
        self.fields(&s.fields, &format!("struct {name}"), is_pub(&s.vis));
        visit::visit_item_struct(self, s);
    }

    fn visit_item_enum(&mut self, e: &'ast ItemEnum) {
        if is_test(&e.attrs) {
            return;
        }
        let public = is_pub(&e.vis);
        for v in &e.variants {
            let owner = format!("enum {}::{}", e.ident, v.ident);
            for f in &v.fields {
                if is_bool(&f.ty) {
                    self.hit(
                        Kind::BoolParamOrField,
                        v.ident.span(),
                        format!("{owner}: bool field"),
                    );
                }
                if public && has_primitive(&f.ty) {
                    self.hit(
                        Kind::PrimitiveApi,
                        v.ident.span(),
                        format!("{owner}: primitive payload"),
                    );
                }
            }
        }
        visit::visit_item_enum(self, e);
    }

    fn visit_item_const(&mut self, c: &'ast syn::ItemConst) {
        if is_pub(&c.vis) && has_primitive(&c.ty) {
            self.hit(
                Kind::PrimitiveApi,
                c.ident.span(),
                format!("pub const {}: primitive", c.ident),
            );
        }
        visit::visit_item_const(self, c);
    }

    fn visit_item_static(&mut self, s: &'ast syn::ItemStatic) {
        if is_pub(&s.vis) && has_primitive(&s.ty) {
            self.hit(
                Kind::PrimitiveApi,
                s.ident.span(),
                format!("pub static {}: primitive", s.ident),
            );
        }
        visit::visit_item_static(self, s);
    }

    fn visit_item_type(&mut self, t: &'ast syn::ItemType) {
        if is_pub(&t.vis) && has_primitive(&t.ty) {
            self.hit(
                Kind::PrimitiveApi,
                t.ident.span(),
                format!("pub type {} = primitive", t.ident),
            );
        }
        visit::visit_item_type(self, t);
    }

    fn visit_impl_item_const(&mut self, c: &'ast syn::ImplItemConst) {
        if !self.in_trait_impl && is_pub(&c.vis) && has_primitive(&c.ty) {
            self.hit(
                Kind::PrimitiveApi,
                c.ident.span(),
                format!("pub const {}: primitive", c.ident),
            );
        }
        visit::visit_impl_item_const(self, c);
    }

    fn visit_expr_cast(&mut self, c: &'ast syn::ExprCast) {
        self.hit(Kind::AsCast, c.as_token.span, "as cast");
        visit::visit_expr_cast(self, c);
    }

    fn visit_expr_unsafe(&mut self, u: &'ast syn::ExprUnsafe) {
        self.hit(Kind::UnsafeCode, u.unsafe_token.span, "unsafe block");
        visit::visit_expr_unsafe(self, u);
    }
}

/// The positive control: a fixture with a known count of every kind. If the
/// scanner does not find exactly these, it is broken, and a clean result on
/// the real tree would mean nothing.
pub(crate) const FIXTURE: &str = r#"
pub struct Leak(pub u8);
pub struct Open { pub n: u32, flag: bool }
pub enum Wire { Raw(u16), Fine(Open) }
pub fn raw(x: u32) -> bool { x as u8 == 0 }
pub fn on(cb: impl Fn()) {}
pub fn each<F: FnMut()>(f: F) {}
pub struct Q;
impl Q {
    pub fn get(&self) -> u64 { 0 }
    pub(crate) fn private(x: u32, y: bool) {}
}
impl Default for Q { fn default() -> Self { Q } }
#[expect(dead_code, reason = "fixture")]
fn g() { unsafe {} }
unsafe impl Send for Leak {}
pub trait Sink { fn put(&mut self, q: Q); }
pub const LIMIT: u32 = 3;
pub type Id = u64;
pub fn bytes() -> impl Iterator<Item = u8> { core::iter::empty() }
pub fn sink(s: &mut dyn Sink) {}
pub fn feed(i: impl Iterator<Item = Q>) {}
impl From<u32> for Q { fn from(_: u32) -> Self { Q } }
impl core::ops::Deref for Q { type Target = u64; fn deref(&self) -> &u64 { &0 } }
#[cfg(test)]
mod tests {
    pub fn not_counted(x: u32, b: bool) -> u8 { x as u8 }
}
"#;

/// What [`FIXTURE`] must count, kind by kind.
pub(crate) const FIXTURE_EXPECTED: [(Kind, usize); 8] = [
    // Open.n, Wire::Raw, raw(), From<u32>, Deref::Target = u64, LIMIT, Id,
    // bytes() -> impl Iterator<Item = u8>
    (Kind::PrimitiveApi, 8),
    // Q::get
    (Kind::Seam, 1),
    // Leak
    (Kind::PubNewtypeField, 1),
    // Open.flag, private(y)
    (Kind::BoolParamOrField, 2),
    (Kind::AsCast, 1),
    (Kind::ExpectAttr, 1),
    // the block, the impl
    (Kind::UnsafeCode, 2),
    // on(), each(), sink(&mut dyn Sink) -- not feed(impl Iterator)
    (Kind::CallbackParam, 3),
];

/// Runs the control. `Err` names every kind the scanner miscounted.
pub(crate) fn control() -> anyhow::Result<()> {
    let hits = scan(FIXTURE, &["get".to_string()])?;
    let got = count(&hits);
    let wrong: Vec<String> = FIXTURE_EXPECTED
        .iter()
        .filter(|(k, n)| got.get(k) != Some(n))
        .map(|(k, n)| {
            format!(
                "{} counted {} (expected {n})",
                k.key(),
                got.get(k).copied().unwrap_or(0)
            )
        })
        .collect();
    if wrong.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "positive control failed, the scanner is broken: {}",
            wrong.join("; ")
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_scanner_counts_the_fixture_exactly() {
        super::control().expect("control");
    }
}
