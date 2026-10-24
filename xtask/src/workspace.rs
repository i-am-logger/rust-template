//! The workspace as the gates see it: members from `cargo metadata`, roles
//! from `layers.toml`, budgets from `budgets.toml`, and the files under each.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// What a crate is for. The role decides which gates apply.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    /// The type system: no_std, no I/O, no primitives in its API.
    Domain,
    /// The machine: no_std, sans-IO, no callbacks, no primitives in its API.
    Core,
    /// I/O, platform and GPU drivers.
    Adapter,
    /// A binary.
    Bin,
    /// Build and gate tooling.
    Tool,
    /// The only role that may contain `unsafe` (pillar 6). Off unless declared.
    Quarantine,
}

impl Role {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Domain => "domain",
            Self::Core => "core",
            Self::Adapter => "adapter",
            Self::Bin => "bin",
            Self::Tool => "tool",
            Self::Quarantine => "quarantine",
        }
    }

    /// Domain and core: the roles held to pillars 1, 2 and 5.
    pub(crate) fn is_pure(self) -> bool {
        matches!(self, Self::Domain | Self::Core)
    }
}

/// `layers.toml`.
#[derive(Debug, Deserialize)]
pub(crate) struct Layers {
    pub(crate) crates: BTreeMap<String, Role>,
    pub(crate) roles: BTreeMap<String, RoleRule>,
}

/// What one role may depend on.
#[derive(Debug, Deserialize)]
pub(crate) struct RoleRule {
    pub(crate) may_depend_on: Vec<Role>,
    pub(crate) external: Vec<String>,
}

/// A dependency as `cargo metadata` declares it.
#[derive(Debug, Deserialize)]
pub(crate) struct Dep {
    pub(crate) name: String,
    pub(crate) kind: Option<String>,
}

/// A workspace member as `cargo metadata` reports it.
#[derive(Debug, Deserialize)]
pub(crate) struct Member {
    pub(crate) name: String,
    pub(crate) manifest_path: PathBuf,
    pub(crate) dependencies: Vec<Dep>,
    pub(crate) features: BTreeMap<String, Vec<String>>,
}

impl Member {
    pub(crate) fn dir(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(Path::new("."))
    }
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Member>,
    workspace_root: PathBuf,
}

/// Everything the gates read.
#[derive(Debug)]
pub(crate) struct Workspace {
    pub(crate) root: PathBuf,
    pub(crate) members: Vec<Member>,
    pub(crate) layers: Layers,
}

/// The workspace root and its members, from `cargo metadata` alone -- for
/// `xtask init`, which runs before there is a layers.toml.
pub(crate) fn members() -> Result<(PathBuf, Vec<Member>)> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .context("running cargo metadata")?;
    if !out.status.success() {
        bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let meta: Metadata = serde_json::from_slice(&out.stdout).context("parsing cargo metadata")?;
    Ok((meta.workspace_root, meta.packages))
}

impl Workspace {
    pub(crate) fn load() -> Result<Self> {
        let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["metadata", "--format-version", "1", "--no-deps"])
            .output()
            .context("running cargo metadata")?;
        if !out.status.success() {
            bail!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let meta: Metadata =
            serde_json::from_slice(&out.stdout).context("parsing cargo metadata")?;
        let layers = read_toml(&meta.workspace_root.join("layers.toml"))?;
        Ok(Self {
            root: meta.workspace_root,
            members: meta.packages,
            layers,
        })
    }

    /// The role `layers.toml` gives a crate, if any.
    pub(crate) fn role(&self, name: &str) -> Option<Role> {
        self.layers.crates.get(name).copied()
    }

    pub(crate) fn is_member(&self, name: &str) -> bool {
        self.members.iter().any(|m| m.name == name)
    }

    /// A path relative to the root, for messages.
    pub(crate) fn rel<'a>(&self, p: &'a Path) -> std::path::Display<'a> {
        p.strip_prefix(&self.root).unwrap_or(p).display()
    }

    /// Every file under the root with one of `exts`, skipping build output
    /// and tool state.
    pub(crate) fn files(&self, exts: &[&str]) -> Result<Vec<PathBuf>> {
        files_under(&self.root, exts)
    }
}

/// Directories no gate looks into: build output, tool state, vendored trees,
/// and test fixtures -- data a test reads, not the project's own documents.
const SKIP_DIRS: &[&str] = &[
    "target",
    ".git",
    ".devenv",
    ".direnv",
    "node_modules",
    "fixture",
    "fixtures",
];

pub(crate) fn files_under(dir: &Path, exts: &[&str]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).with_context(|| format!("reading {}", d.display()))? {
            let path = entry?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if !SKIP_DIRS.contains(&name) && !name.starts_with(".devenv") {
                    stack.push(path);
                }
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| exts.contains(&e))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

pub(crate) fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}
