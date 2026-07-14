use super::support::load_lock;
use crate::install::{marker_for, read_marker, validate_destination_location};
use crate::lockfile::Mode;
use crate::output::Envelope;
use crate::scope::Scope;
use crate::transaction::remove_destination_and_lock;
use anyhow::{bail, Context, Result};
use serde_json::json;

pub(super) fn run(scope: &Scope, name: &str, dry_run: bool) -> Result<(Envelope, Vec<String>)> {
    let mut lock = load_lock(scope)?;
    let entry = lock
        .skills
        .get(name)
        .cloned()
        .with_context(|| format!("lock entry `{name}` does not exist"))?;
    let destination = scope.skills_dir.join(&entry.destination);
    let remove_destination = if entry.mode == Mode::Vendored && destination.exists() {
        validate_destination_location(scope, &entry)?;
        crate::digest::inspect_tree(&destination)
            .with_context(|| format!("refusing to delete unsafe destination for `{name}`"))?;
        let marker = read_marker(&destination)
            .with_context(|| format!("refusing to delete unmanaged destination for `{name}`"))?;
        if marker != marker_for(scope, name, &entry)? {
            bail!("refusing to delete destination for `{name}` because its marker does not match");
        }
        Some(destination.as_path())
    } else {
        None
    };
    lock.skills.remove(name);
    let bytes = lock.yaml()?;
    let mut envelope = Envelope::new(scope);
    envelope.changes.push(json!({
        "action": "remove",
        "name": name,
        "destination_removed": remove_destination.is_some(),
        "local_files_remain": entry.mode == Mode::Local,
        "dry_run": dry_run,
    }));
    if !dry_run {
        remove_destination_and_lock(remove_destination, &scope.lock_file, &bytes)?;
    }
    let detail = if entry.mode == Mode::Local {
        "local files remain"
    } else if remove_destination.is_some() {
        "managed files removed"
    } else {
        "destination was already missing"
    };
    Ok((
        envelope,
        vec![format!(
            "{} `{name}` from lock ({detail})",
            if dry_run { "would remove" } else { "removed" }
        )],
    ))
}
