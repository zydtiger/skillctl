use super::support::{load_lock, select_names};
use crate::install::{compare_trees, validate_destination_location};
use crate::lockfile::Mode;
use crate::output::Envelope;
use crate::scope::Scope;
use crate::source::acquire;
use anyhow::{bail, Result};
use serde_json::json;

pub(super) fn run(scope: &Scope, selected: Option<&str>) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let names = select_names(&lock, selected)?;
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    for name in names {
        let entry = lock.skills.get(&name).expect("selected entry");
        if entry.mode == Mode::Local {
            if selected.is_some() {
                bail!("cannot diff local entry `{name}` against a pinned source");
            }
            continue;
        }
        validate_destination_location(scope, entry)?;
        let installed = scope.skills_dir.join(&entry.destination);
        if !installed.exists() {
            bail!("cannot diff missing destination for `{name}`");
        }
        let source = entry.source.as_ref().expect("validated lock");
        let resolved = entry.resolved.as_ref().expect("validated lock");
        let snapshot = acquire(
            &source.repository,
            &source.selector()?,
            Some(&resolved.commit),
        )?;
        if snapshot.digest != resolved.digest {
            bail!("pinned source digest for `{name}` no longer matches the lock");
        }
        let changes = compare_trees(&installed, &snapshot.root)?;
        if changes.is_empty() {
            lines.push(format!("{name}: no differences"));
        }
        for change in changes {
            lines.push(format!("{name}: {} {}", change.change, change.path));
            envelope.changes.push(json!({
                "name": name,
                "path": change.path,
                "change": change.change,
            }));
        }
    }
    Ok((envelope, lines))
}
