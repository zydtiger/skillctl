use crate::install::validate_scope_layout;
use crate::lockfile::LockFile;
use crate::output::Envelope;
use crate::scope::Scope;
use crate::transaction::atomic_write;
use anyhow::{bail, Result};
use serde_json::json;
use std::fs;

pub(super) fn run(scope: &Scope, dry_run: bool) -> Result<(Envelope, Vec<String>)> {
    validate_scope_layout(scope)?;
    if scope.lock_file.exists() {
        bail!(
            "refusing to overwrite existing lock {}",
            scope.lock_file.display()
        );
    }
    let lock = LockFile::empty();
    let mut envelope = Envelope::new(scope);
    envelope.changes.push(json!({
        "action": "initialize",
        "path": scope.lock_file,
        "version": lock.version,
        "dry_run": dry_run,
    }));
    if !dry_run {
        fs::create_dir_all(&scope.skills_dir)?;
        atomic_write(&scope.lock_file, &lock.yaml()?)?;
    }
    Ok((
        envelope,
        vec![if dry_run {
            format!(
                "would initialize {} scope at {}",
                scope.label(),
                scope.lock_file.display()
            )
        } else {
            format!(
                "initialized {} scope at {}",
                scope.label(),
                scope.lock_file.display()
            )
        }],
    ))
}
