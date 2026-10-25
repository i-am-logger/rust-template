//! `cargo xtask sources`: the research sources manifest, and every citation.
//!
//! `docs/research/sources/MANIFEST.toml` lists each source a note or a
//! comment cites, by key, with its sha256:
//!
//! * `retrieved`: committed under `docs/research/sources/`, re-hashed here;
//! * `external`: read and hashed, but not committed (licence or size);
//! * `gated`: never obtained, so it may not be cited at all.
//!
//! Every file in the sources directory must be a retrieved entry's, and every
//! `[[source: <key>]]` in a `.md` or `.rs` file must name a citable entry.
//! A key is lowercase letters, digits, `.`, `_` and `-`. The one thing after
//! the marker that is not a citation is an example in angle brackets on one
//! line; anything else that is not a key fails, naming the file and line: an
//! uppercase letter or other character outside the alphabet, a space (two
//! keys, or a locator inside the brackets), a key split across lines, an empty
//! citation, or one with no `]]` before the next citation or the end of the
//! file. Space around the key, a line break after the marker included, is
//! not part of it.

use std::collections::BTreeMap;
use std::fs;

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::report::Report;
use crate::workspace::{Workspace, read_toml};

const GATE: &str = "sources";
const DIR: &str = "docs/research/sources";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Retrieved,
    External,
    Gated,
}

#[derive(Debug, Deserialize)]
struct Source {
    status: Status,
    sha256: Option<String>,
    file: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Manifest {
    #[serde(default)]
    sources: BTreeMap<String, Source>,
}

/// The citation marker, built so this file does not contain it literally.
const MARK: &str = concat!("[", "[source:");

fn is_key(k: &str) -> bool {
    !k.is_empty()
        && k.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

/// What follows one marker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cite<'a> {
    /// A key, to look up in the manifest.
    Key(&'a str),
    /// Not a key: what follows the marker, and what is wrong with it.
    Malformed(&'a str, Flaw),
}

/// Why the text after a marker is not a key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flaw {
    /// Nothing between the marker and `]]`.
    Empty,
    /// A line break inside: a key split across lines.
    Split,
    /// A space inside: two keys, or a locator inside the brackets.
    Space,
    /// A character outside the key alphabet, such as an uppercase letter.
    Alphabet,
    /// No `]]` before the next marker or the end of the file.
    Unterminated,
}

/// One marker in a text, with the line it starts on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Found<'a> {
    line: usize,
    cite: Cite<'a>,
}

impl Flaw {
    fn says(self) -> &'static str {
        match self {
            Self::Empty => "it is empty",
            Self::Split => "it is split across lines",
            Self::Space => {
                "it has a space inside (one key per citation; the clause goes after `]]`)"
            }
            Self::Alphabet => "it has a character outside the key alphabet",
            Self::Unterminated => "it has no `]]` before the next citation or the end of the file",
        }
    }
}

/// An example in angle brackets, such as `<key>`, on one line: not a citation.
fn is_example(body: &str) -> bool {
    body.starts_with('<') && body.ends_with('>') && !body.contains('\n')
}

/// What the text between a marker and its `]]` is.
fn classify(body: &str) -> Option<Cite<'_>> {
    let body = body.trim();
    let flaw = if is_example(body) {
        return None;
    } else if is_key(body) {
        return Some(Cite::Key(body));
    } else if body.is_empty() {
        Flaw::Empty
    } else if body.contains('\n') {
        Flaw::Split
    } else if body.contains(char::is_whitespace) {
        Flaw::Space
    } else {
        Flaw::Alphabet
    };
    Some(Cite::Malformed(body, flaw))
}

/// Every marker in a text but an example: each a key or malformed. Nothing
/// after a marker is skipped silently.
fn citations(text: &str) -> Vec<Found<'_>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = text[at..].find(MARK) {
        let line = text[..at + i].matches('\n').count() + 1;
        at += i + MARK.len();
        let rest = &text[at..];
        let next = rest.find(MARK).unwrap_or(rest.len());
        let cite = match rest[..next].find("]]") {
            Some(end) => {
                at += end + 2;
                classify(&rest[..end])
            }
            None => {
                let body = rest[..next].trim();
                let first = body.lines().next().unwrap_or("");
                Some(Cite::Malformed(first, Flaw::Unterminated))
            }
        };
        out.extend(cite.map(|cite| Found { line, cite }));
    }
    out
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn run(ws: &Workspace, r: &mut Report) -> Result<()> {
    control()?;
    let dir = ws.root.join(DIR);
    let manifest: Manifest = read_toml(&dir.join("MANIFEST.toml"))?;
    entries(ws, r, &manifest)?;
    accounted(ws, r, &manifest)?;
    let mut cited = 0;
    for file in ws.files(&["md", "rs"])? {
        let text = fs::read_to_string(&file)?;
        for found in citations(&text) {
            let place = format!("{}:{}", ws.rel(&file), found.line);
            let key = match found.cite {
                Cite::Key(key) => key,
                Cite::Malformed(body, flaw) => {
                    r.fail(GATE, place, malformed(body, flaw));
                    continue;
                }
            };
            cited += 1;
            match manifest.sources.get(key) {
                None => r.fail(
                    GATE,
                    place,
                    format!("cites `{key}`, which MANIFEST.toml does not list"),
                ),
                Some(s) if s.status == Status::Gated => r.fail(
                    GATE,
                    place,
                    format!("cites `{key}`, which is gated: never obtained, so not citable"),
                ),
                Some(_) => {}
            }
        }
    }
    r.checked(
        GATE,
        format!("{} sources, {cited} citations", manifest.sources.len()),
    );
    Ok(())
}

/// The finding for a malformed citation: what it says, shortened, and why it
/// is not a key.
fn malformed(body: &str, flaw: Flaw) -> String {
    let shown: String = body.chars().take(60).collect();
    let more = if shown.len() < body.len() { "..." } else { "" };
    format!(
        "cites `{}{more}`, which is not a key: {}. A key is lowercase letters, \
         digits, `.`, `_` and `-`; an example goes in angle brackets",
        shown.escape_debug(),
        flaw.says()
    )
}

fn entries(ws: &Workspace, r: &mut Report, m: &Manifest) -> Result<()> {
    for (key, s) in &m.sources {
        let place = format!("MANIFEST.toml [sources.{key}]");
        if s.status != Status::Gated && !s.sha256.as_deref().is_some_and(is_sha256) {
            r.fail(GATE, &place, "needs a sha256 of 64 lowercase hex digits");
        }
        if s.status != Status::Retrieved {
            continue;
        }
        let Some(file) = &s.file else {
            r.fail(GATE, &place, "is retrieved but names no file");
            continue;
        };
        let path = ws.root.join(DIR).join(file);
        let Ok(bytes) = fs::read(&path) else {
            r.fail(
                GATE,
                &place,
                format!("names `{file}`, which is not in {DIR}"),
            );
            continue;
        };
        let got = format!("{:x}", Sha256::digest(&bytes));
        if s.sha256.as_deref() != Some(got.as_str()) {
            r.fail(
                GATE,
                &place,
                format!("`{file}` hashes to {got}, not the pinned sha256"),
            );
        }
    }
    Ok(())
}

fn accounted(ws: &Workspace, r: &mut Report, m: &Manifest) -> Result<()> {
    let dir = ws.root.join(DIR);
    let listed: Vec<&str> = m
        .sources
        .values()
        .filter_map(|s| s.file.as_deref())
        .collect();
    for f in any_file(&dir).context("listing sources")? {
        let name = f
            .strip_prefix(&dir)
            .unwrap_or(&f)
            .to_string_lossy()
            .to_string();
        if name != "MANIFEST.toml" && name != "README.md" && !listed.contains(&name.as_str()) {
            r.fail(
                GATE,
                ws.rel(&f).to_string(),
                "is in the sources directory but in no MANIFEST.toml entry",
            );
        }
    }
    Ok(())
}

/// Every file under `dir`, whatever its extension.
fn any_file(dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    Ok(out)
}

/// The positive control: an unknown citation, a malformed one and a bad hash
/// must be seen.
fn control() -> Result<()> {
    let fixture = format!("x {MARK} nope]] y {MARK} <key>]]\nz {MARK} Nope]]");
    anyhow::ensure!(
        citations(&fixture)
            == [
                Found {
                    line: 1,
                    cite: Cite::Key("nope")
                },
                Found {
                    line: 2,
                    cite: Cite::Malformed("Nope", Flaw::Alphabet)
                }
            ],
        "positive control failed: citation scanner"
    );
    anyhow::ensure!(
        !is_sha256("abc"),
        "positive control failed: sha256 check accepts a short digest"
    );
    anyhow::ensure!(
        is_sha256(&format!("{:x}", Sha256::digest(b""))),
        "positive control failed: sha256 check rejects a real digest"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bad(line: usize, body: &str, flaw: Flaw) -> Found<'_> {
        Found {
            line,
            cite: Cite::Malformed(body, flaw),
        }
    }

    fn key(line: usize, k: &str) -> Found<'_> {
        Found {
            line,
            cite: Cite::Key(k),
        }
    }

    #[test]
    fn an_uppercase_key_is_malformed() {
        let t = format!("Per {MARK} Foo]] section 1.");
        assert_eq!(citations(&t), [bad(1, "Foo", Flaw::Alphabet)]);
    }

    #[test]
    fn a_comma_is_outside_the_alphabet() {
        let t = format!("{MARK} a,b]]");
        assert_eq!(citations(&t), [bad(1, "a,b", Flaw::Alphabet)]);
    }

    #[test]
    fn two_keys_or_a_locator_inside_are_malformed() {
        let t = format!("{MARK} a, b]] and {MARK} foo section-3]]");
        assert_eq!(
            citations(&t),
            [
                bad(1, "a, b", Flaw::Space),
                bad(1, "foo section-3", Flaw::Space)
            ]
        );
    }

    #[test]
    fn a_key_split_across_lines_is_malformed() {
        let t = format!("x\n{MARK} beautifulrust-\nskill]]");
        assert_eq!(
            citations(&t),
            [bad(2, "beautifulrust-\nskill", Flaw::Split)]
        );
    }

    #[test]
    fn a_rust_comment_wrap_inside_the_brackets_is_malformed() {
        let t = format!("//! Per {MARK}\n//! foo]]");
        assert_eq!(citations(&t), [bad(1, "//! foo", Flaw::Space)]);
    }

    #[test]
    fn an_empty_citation_is_malformed() {
        let t = format!("{MARK} ]] and {MARK}]]");
        assert_eq!(
            citations(&t),
            [bad(1, "", Flaw::Empty), bad(1, "", Flaw::Empty)]
        );
    }

    #[test]
    fn an_unterminated_citation_at_the_end_is_malformed() {
        let t = format!("ok\nPer {MARK} foo\nmore text");
        assert_eq!(citations(&t), [bad(2, "foo", Flaw::Unterminated)]);
    }

    #[test]
    fn an_unterminated_citation_does_not_swallow_the_next() {
        let t = format!("{MARK} foo] then\n{MARK} bar]]");
        assert_eq!(
            citations(&t),
            [bad(1, "foo] then", Flaw::Unterminated), key(2, "bar")]
        );
    }

    #[test]
    fn keys_placeholders_and_surrounding_space_are_accepted() {
        let t = format!("{MARK} <key>]] {MARK}ok-1.2_x]]\n{MARK}\n  wrapped ]]");
        assert_eq!(citations(&t), [key(1, "ok-1.2_x"), key(2, "wrapped")]);
    }

    #[test]
    fn a_placeholder_split_across_lines_is_malformed() {
        let t = format!("{MARK} <a\nb>]]");
        assert_eq!(citations(&t), [bad(1, "<a\nb>", Flaw::Split)]);
    }
}
