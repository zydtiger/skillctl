use super::support::{ensure_replace_allowed, install_prepared, load_lock, select_names};
use crate::install::{marker_for, read_marker, write_marker};
use crate::lockfile::{Mode, Resolved};
use crate::output::Envelope;
use crate::scope::Scope;
use crate::source::acquire_many;
use crate::transaction::atomic_write;
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::collections::{BTreeMap, HashSet};

pub(super) fn sync(
    scope: &Scope,
    selected: &[String],
    force: bool,
    dry_run: bool,
) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let names = select_names(&lock, selected)?;

    // Reject an explicitly named `mode: local` entry before any network
    // acquisition starts: local files are project-owned, so syncing one is
    // always refused regardless of whether its source is even reachable.
    // The bare, all-entries form skips local entries silently instead, in
    // the loop below.
    if !selected.is_empty() {
        for name in &names {
            if lock.skills.get(name).expect("selected entry").mode == Mode::Local {
                bail!("cannot sync local entry `{name}`; local files are project-owned");
            }
        }
    }

    // Build the distinct (repository, pinned commit) groups in the names'
    // first-encounter order, the same grouping `status`'s upstream_states
    // uses, so several entries vendored from the same source at the same pin
    // share a single acquisition. Every group is then acquired concurrently
    // (bounded inside `acquire_many`), all results are collected, and only
    // then is the first-encounter order walked to propagate the first error
    // in that order. This keeps the surfaced error identical to the prior
    // one-acquisition-per-entry, first-broken-source behavior regardless of
    // which acquisition happens to finish first on the wall clock. Every
    // acquisition happens here, strictly before any destination, marker, or
    // lock mutation below, so a failed acquisition still aborts the run with
    // nothing written.
    let mut group_order = Vec::new();
    let mut seen = HashSet::new();
    for name in &names {
        let entry = lock.skills.get(name).expect("selected entry");
        if entry.mode == Mode::Local {
            continue;
        }
        let source = entry.source.as_ref().expect("validated lock");
        let resolved = entry.resolved.as_ref().expect("validated lock");
        let key = (source.repository.clone(), resolved.commit.clone());
        if seen.insert(key.clone()) {
            group_order.push(key);
        }
    }
    let group_results = acquire_many(&group_order);
    let mut acquired = BTreeMap::new();
    let mut first_error = None;
    for (key, result) in group_order.into_iter().zip(group_results) {
        match result {
            Ok(repository) => {
                acquired.insert(key, repository);
            }
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }

    let mut prepared = Vec::new();
    let mut skipped = Vec::new();
    for name in names {
        let entry = lock.skills.get(&name).expect("selected entry");
        if entry.mode == Mode::Local {
            // Only reachable for the bare, all-entries form: an explicitly
            // named local entry was already rejected above, before
            // acquisition.
            skipped.push(name);
            continue;
        }
        let source = entry.source.as_ref().expect("validated lock");
        let resolved = entry.resolved.as_ref().expect("validated lock");
        let repo = acquired
            .get(&(source.repository.clone(), resolved.commit.clone()))
            .expect("acquired source group");
        let snapshot = repo.select(&source.selector()?)?;
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
    selected: &[String],
    force: bool,
    dry_run: bool,
) -> Result<(Envelope, Vec<String>)> {
    let mut lock = load_lock(scope)?;
    let names = select_names(&lock, selected)?;
    let original = lock.clone();

    // Reject an explicitly named `mode: local` entry before any network
    // acquisition starts: local files are project-owned, so updating one is
    // always refused regardless of whether its source is even reachable.
    // The bare, all-entries form leaves local entries untouched silently
    // instead, in the loop below.
    if !selected.is_empty() {
        for name in &names {
            if original.skills.get(name).expect("selected entry").mode == Mode::Local {
                bail!("cannot update local entry `{name}`; local files are project-owned");
            }
        }
    }

    // Build the distinct (repository, followed reference) groups in the
    // names' first-encounter order, the same grouping `status`'s
    // upstream_states uses, so several entries that track the same branch or
    // tag share a single acquisition. Every group is then acquired
    // concurrently (bounded inside `acquire_many`), all results are
    // collected, and only then is the first-encounter order walked to
    // propagate the first error in that order. This keeps the surfaced error
    // identical to the prior one-acquisition-per-entry, first-broken-source
    // behavior regardless of which acquisition happens to finish first on the
    // wall clock. Every acquisition happens here, strictly before any
    // destination, marker, or lock mutation below, so a failed acquisition
    // still aborts the run with nothing written.
    let mut group_order = Vec::new();
    let mut seen = HashSet::new();
    for name in &names {
        let old = original.skills.get(name).expect("selected entry");
        if old.mode == Mode::Local {
            continue;
        }
        let source = old.source.as_ref().expect("validated lock");
        let key = (source.repository.clone(), source.reference.clone());
        if seen.insert(key.clone()) {
            group_order.push(key);
        }
    }
    let group_results = acquire_many(&group_order);
    let mut acquired = BTreeMap::new();
    let mut first_error = None;
    for (key, result) in group_order.into_iter().zip(group_results) {
        match result {
            Ok(repository) => {
                acquired.insert(key, repository);
            }
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }

    let mut prepared = Vec::new();
    let mut pin_only = Vec::new();
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    for name in names {
        let old = original.skills.get(&name).expect("selected entry");
        if old.mode == Mode::Local {
            // Only reachable for the bare, all-entries form: an explicitly
            // named local entry was already rejected above, before
            // acquisition.
            lines.push(format!("left local entry `{name}` untouched"));
            continue;
        }
        let source = old.source.as_ref().expect("validated lock");
        let repo = acquired
            .get(&(source.repository.clone(), source.reference.clone()))
            .expect("acquired source group");
        let snapshot = repo.select(&source.selector()?)?;
        let resolved = old.resolved.as_ref().expect("validated lock");
        // A pin-only advance assumes the destination already holds the pinned
        // content, so it only applies to an installed destination. A missing
        // one still needs a real install.
        let installed = scope.skills_dir.join(&old.destination).exists();
        if snapshot.digest == resolved.digest && installed {
            if snapshot.commit == resolved.commit {
                lines.push(format!("{name}: already up to date at {}", resolved.commit));
                envelope.changes.push(json!({
                    "action": "no-op",
                    "name": name,
                    "commit": resolved.commit,
                }));
            } else {
                // The selected skill's content is unchanged, so the installed
                // destination already matches the advanced commit. Record the
                // new pin without staging or replacing anything.
                pin_only.push((name, snapshot));
            }
            continue;
        }
        ensure_replace_allowed(scope, &name, old, &snapshot, force)?;
        prepared.push((name, snapshot));
    }
    for (name, snapshot) in &pin_only {
        let updated = lock.skills.get_mut(name).expect("pin-only entry");
        updated.resolved = Some(Resolved {
            commit: snapshot.commit.clone(),
            digest: snapshot.digest.clone(),
        });
        lock.validate()?;
        envelope.changes.push(json!({
            "action": "pin-only",
            "name": name,
            "commit": snapshot.commit,
            "digest": snapshot.digest,
            "dry_run": dry_run,
        }));
        lines.push(format!(
            "{} `{name}` to {} (content unchanged)",
            if dry_run {
                "would advance pin for"
            } else {
                "advanced pin for"
            },
            snapshot.commit
        ));
    }
    if !dry_run && !pin_only.is_empty() {
        // Commit the pin-only advances before any content update runs, so a
        // later failure cannot leave these entries' markers behind a lock that
        // `install_prepared` already persisted as a side effect.
        commit_pin_only(scope, &lock, &pin_only)?;
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

/// Persist pin-only advances so the lock and every refreshed marker move
/// together.
///
/// Markers are refreshed first and rolled back if the lock write fails, because
/// a marker is only consistent against the pin the lock actually records. The
/// marker is excluded from the content digest, so rewriting it in place leaves
/// the installed tree's identity unchanged.
fn commit_pin_only(
    scope: &Scope,
    lock: &crate::lockfile::LockFile,
    pin_only: &[(String, crate::source::Snapshot)],
) -> Result<()> {
    let mut written = Vec::new();
    let result = (|| {
        for (name, _) in pin_only {
            let entry = lock.skills.get(name).expect("pin-only entry");
            let destination = scope.skills_dir.join(&entry.destination);
            if !destination.exists() {
                continue;
            }
            let previous = read_marker(&destination).ok();
            write_marker(&destination, &marker_for(scope, name, entry)?)?;
            written.push((destination, previous));
        }
        atomic_write(&scope.lock_file, &lock.yaml()?)
    })();
    let Err(error) = result else {
        return Ok(());
    };
    let mut stranded = Vec::new();
    for (destination, previous) in written {
        let restored = match &previous {
            Some(previous) => write_marker(&destination, previous),
            // The entry had no readable marker to restore, so it was already
            // invalid before this run and stays that way.
            None => Ok(()),
        };
        if restored.is_err() {
            stranded.push(destination.display().to_string());
        }
    }
    if stranded.is_empty() {
        return Err(error);
    }
    Err(error).context(format!(
        "pin-only write failed and marker rollback also failed for {}; run `skillctl check`",
        stranded.join(", ")
    ))
}
