//! `cargo xtask layers`: every dependency edge against `layers.toml`
//! (pillar 3). A workspace dependency must point at a role the depending
//! role may use; an external one must be on the role's allowlist. Dev
//! dependencies are not checked: tests may use what the code may not.

use anyhow::Result;

use crate::report::Report;
use crate::workspace::{Role, RoleRule, Workspace};

const GATE: &str = "layers";

/// Whether `role` may depend on a crate of role `dep`.
fn may_use(rule: &RoleRule, dep: Role) -> bool {
    rule.may_depend_on.contains(&dep)
}

/// Whether `role` may depend on the external crate `name`.
fn may_use_external(rule: &RoleRule, name: &str) -> bool {
    rule.external.iter().any(|e| e == "*" || e == name)
}

pub(crate) fn run(ws: &Workspace, r: &mut Report) -> Result<()> {
    control()?;
    let mut edges = 0_usize;
    for m in &ws.members {
        let Some(role) = ws.role(&m.name) else {
            continue;
        };
        let Some(rule) = ws.layers.roles.get(role.name()) else {
            r.fail(
                GATE,
                "layers.toml",
                format!("has no [roles.{}]", role.name()),
            );
            continue;
        };
        for d in m
            .dependencies
            .iter()
            .filter(|d| d.kind.as_deref() != Some("dev"))
        {
            edges += 1;
            match ws.role(&d.name) {
                Some(dep_role) if !may_use(rule, dep_role) => r.fail(
                    GATE,
                    &m.name,
                    format!(
                        "a {} crate may not depend on `{}` (a {} crate); allowed: {:?}",
                        role.name(),
                        d.name,
                        dep_role.name(),
                        rule.may_depend_on
                    ),
                ),
                None if !ws.is_member(&d.name) && !may_use_external(rule, &d.name) => r.fail(
                    GATE,
                    &m.name,
                    format!(
                        "a {} crate may not depend on the external crate `{}`; allowlist: {:?}",
                        role.name(),
                        d.name,
                        rule.external
                    ),
                ),
                _ => {}
            }
        }
    }
    r.checked(GATE, format!("{edges} dependency edges"));
    Ok(())
}

/// The positive control: the rule must reject edges known to be wrong.
fn control() -> Result<()> {
    let core = RoleRule {
        may_depend_on: vec![Role::Domain],
        external: vec![],
    };
    anyhow::ensure!(
        may_use(&core, Role::Domain),
        "positive control failed: core -> domain rejected"
    );
    anyhow::ensure!(
        !may_use(&core, Role::Bin),
        "positive control failed: core -> bin accepted"
    );
    anyhow::ensure!(
        !may_use_external(&core, "tokio"),
        "positive control failed: an unlisted external accepted"
    );
    Ok(())
}
