use super::support::{ensure_replace_allowed, install_prepared, load_lock, select_names};
use crate::lockfile::{Mode, Resolved};
use crate::output::Envelope;
use crate::scope::Scope;
use crate::source::acquire;
use anyhow::{bail, Result};
use serde_json::json;

pub(super) fn sync(
    scope: &Scope,
    selected: Option<&str>,
    force: bool,
    dry_run: bool,
) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let names = select_names(&lock, selected)?;
    let mut prepared = Vec::new();
    let mut skipped = Vec::new();
    for name in names {
        let entry = lock.skills.get(&name).expect("selected entry");
        if entry.mode == Mode::Local {
            if selected.is_some() {
                bail!("cannot sync local entry `{name}`; local files are project-owned");
            }
            skipped.push(name);
            continue;
        }
        let source = entry.source.as_ref().expect("validated lock");
        let resolved = entry.resolved.as_ref().expect("validated lock");
        let snapshot = acquire(
            &source.repository,
            &source.selector()?,
            Some(&resolved.commit),
        )?;
        if snapshot.commit != resolved.commit || snapshot.digest != resolved.digest {
            bail!(
                "pinned snapshot for `{name}` does not match lock: got {} {}, expected {} {}",
                snapshot.commit,
                snapshot.digest,
                resolved.commit,
                resolved.digest
            );
        }
        ensure_replace_allowed(scope, &name, entry, &snapshot, force)?;
        prepared.push((name, snapshot));
    }
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    for (name, snapshot) in &prepared {
        let entry = lock.skills.get(name).expect("prepared entry");
        envelope.changes.push(json!({
            "action": "sync",
            "name": name,
            "commit": snapshot.commit,
            "dry_run": dry_run,
        }));
        if !dry_run {
            install_prepared(scope, name, entry, snapshot, &lock)?;
        }
        lines.push(format!(
            "{} `{name}` to {}",
            if dry_run { "would sync" } else { "synced" },
            snapshot.commit
        ));
    }
    for name in skipped {
        envelope
            .changes
            .push(json!({"action":"skip-local", "name":name}));
        lines.push(format!("left local entry `{name}` untouched"));
    }
    Ok((envelope, lines))
}

pub(super) fn update(
    scope: &Scope,
    selected: Option<&str>,
    force: bool,
    dry_run: bool,
) -> Result<(Envelope, Vec<String>)> {
    let mut lock = load_lock(scope)?;
    let names = select_names(&lock, selected)?;
    let original = lock.clone();
    let mut prepared = Vec::new();
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    for name in names {
        let old = original.skills.get(&name).expect("selected entry");
        if old.mode == Mode::Local {
            if selected.is_some() {
                bail!("cannot update local entry `{name}`; local files are project-owned");
            }
            lines.push(format!("left local entry `{name}` untouched"));
            continue;
        }
        let source = old.source.as_ref().expect("validated lock");
        let snapshot = acquire(
            &source.repository,
            &source.selector()?,
            Some(&source.reference),
        )?;
        let resolved = old.resolved.as_ref().expect("validated lock");
        if snapshot.commit == resolved.commit && snapshot.digest == resolved.digest {
            lines.push(format!("{name}: already up to date at {}", resolved.commit));
            envelope.changes.push(json!({
                "action": "no-op",
                "name": name,
                "commit": resolved.commit,
            }));
            continue;
        }
        ensure_replace_allowed(scope, &name, old, &snapshot, force)?;
        prepared.push((name, snapshot));
    }
    for (name, snapshot) in &prepared {
        let updated = lock.skills.get_mut(name).expect("prepared entry");
        updated.resolved = Some(Resolved {
            commit: snapshot.commit.clone(),
            digest: snapshot.digest.clone(),
        });
        lock.validate()?;
        let entry = lock.skills.get(name).expect("prepared entry");
        envelope.changes.push(json!({
            "action": "update",
            "name": name,
            "commit": snapshot.commit,
            "digest": snapshot.digest,
            "dry_run": dry_run,
        }));
        if !dry_run {
            install_prepared(scope, name, entry, snapshot, &lock)?;
        }
        lines.push(format!(
            "{} `{name}` to {}",
            if dry_run { "would update" } else { "updated" },
            snapshot.commit
        ));
    }
    Ok((envelope, lines))
}
