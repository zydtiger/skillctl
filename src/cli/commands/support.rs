use crate::install::{
    compare_trees, copy_snapshot, entry_state, marker_for, validate_destination_location,
    validate_scope_layout, write_marker,
};
use crate::lockfile::{LockFile, Mode, SkillEntry};
use crate::scope::Scope;
use crate::source::Snapshot;
use crate::transaction::replace_destination_and_lock;
use anyhow::{bail, Result};
use std::collections::HashSet;
use std::fs;

pub(super) fn load_lock(scope: &Scope) -> Result<LockFile> {
    validate_scope_layout(scope)?;
    LockFile::load(&scope.lock_file)
}

/// Resolve the entries a command should operate on. An empty `selected`
/// slice keeps the long-standing bare-invocation meaning of "every lock
/// entry"; a non-empty slice validates that every named entry exists,
/// failing the whole invocation on the first unknown name before any
/// acquisition or mutation happens, and de-duplicates repeats so an entry
/// named more than once is still processed exactly once, in
/// first-occurrence order.
pub(super) fn select_names(lock: &LockFile, selected: &[String]) -> Result<Vec<String>> {
    if selected.is_empty() {
        return Ok(lock.skills.keys().cloned().collect());
    }
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for name in selected {
        if !lock.skills.contains_key(name) {
            bail!("lock entry `{name}` does not exist");
        }
        if seen.insert(name.as_str()) {
            names.push(name.clone());
        }
    }
    Ok(names)
}

pub(super) fn ensure_replace_allowed(
    scope: &Scope,
    name: &str,
    entry: &SkillEntry,
    expected: &Snapshot,
    force: bool,
) -> Result<()> {
    validate_destination_location(scope, entry)?;
    let destination = scope.skills_dir.join(&entry.destination);
    if !destination.exists() {
        return Ok(());
    }
    let state = entry_state(scope, name, entry);
    if state.state == "clean" {
        return Ok(());
    }
    let changes = compare_trees(&destination, &expected.root)
        .map(|changes| {
            changes
                .into_iter()
                .map(|change| format!("{} {}", change.change, change.path))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|_| state.details.join("; "));
    if !force {
        bail!("refusing to replace changed destination for `{name}` without --force: {changes}");
    }
    Ok(())
}

pub(super) fn install_prepared(
    scope: &Scope,
    name: &str,
    entry: &SkillEntry,
    snapshot: &Snapshot,
    lock: &LockFile,
) -> Result<()> {
    validate_destination_location(scope, entry)?;
    let staged = copy_snapshot(&snapshot.root, &scope.skills_dir)?;
    let result = (|| {
        write_marker(&staged, &marker_for(scope, name, entry)?)?;
        let actual = crate::digest::digest_tree(&staged)?;
        let expected = &entry.resolved.as_ref().expect("vendored entry").digest;
        if &actual != expected {
            bail!("staged digest for `{name}` changed unexpectedly");
        }
        replace_destination_and_lock(
            &staged,
            &scope.skills_dir.join(&entry.destination),
            &scope.lock_file,
            &lock.yaml()?,
        )
    })();
    if staged.exists() {
        let _ = fs::remove_dir_all(&staged);
    }
    result
}

pub(super) fn entry_for_destination(destination: &str) -> SkillEntry {
    SkillEntry {
        mode: Mode::Local,
        source: None,
        resolved: None,
        destination: destination.to_owned(),
    }
}
