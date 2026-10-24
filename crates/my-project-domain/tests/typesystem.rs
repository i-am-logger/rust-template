//! Pillar 1, held by the compiler: each illegal program in `ui/fail` must fail
//! to compile with the error pinned in its `.stderr`, and each has a twin in
//! `ui/pass` that differs only in the illegal line and must compile. Without
//! the twin a `compile_fail` case can fail for an unrelated reason -- a typo,
//! a missing import -- and still read as a pass; `cargo xtask docs` checks
//! every case has its twin.

#[test]
fn illegal_programs_do_not_compile() {
    trybuild::TestCases::new().compile_fail("tests/ui/fail/*.rs");
}

#[test]
fn their_twins_do() {
    trybuild::TestCases::new().pass("tests/ui/pass/*.rs");
}
