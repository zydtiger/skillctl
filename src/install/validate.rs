use crate::lockfile::{LockFile, SkillEntry};
use crate::scope::Scope;
use crate::source::validate_skill_tree;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;

pub fn validate_scope_layout(scope: &Scope) -> Result<()> {
    for path in [&scope.agents_dir, &scope.skills_dir, &scope.lock_file] {
        if let Ok(metadata) = fs::symlink_metadata(path) {
            if metadata.file_type().is_symlink() {
                bail!("scope path must not be a symlink: {}", path.display());
            }
        }
    }
    Ok(())
}

pub fn validate_destination_location(scope: &Scope, entry: &SkillEntry) -> Result<()> {
    validate_scope_layout(scope)?;
    let relative = crate::lockfile::safe_relative_path(&entry.destination)?;
    let mut current = scope.skills_dir.clone();
    for component in relative.components() {
        current.push(component);
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                bail!(
                    "destination path traverses symlink at {}",
                    current.display()
                );
            }
        }
    }
    Ok(())
}

pub fn validate_declared_names(scope: &Scope, lock: &LockFile) -> Result<()> {
    let mut names: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (entry_name, entry) in &lock.skills {
        let destination = scope.skills_dir.join(&entry.destination);
        if destination.exists() {
            let metadata = validate_skill_tree(&destination)
                .with_context(|| format!("invalid installed skill `{entry_name}`"))?;
            names
                .entry(metadata.name)
                .or_default()
                .push(entry_name.clone());
        }
    }
    let duplicates: Vec<String> = names
        .into_iter()
        .filter(|(_, entries)| entries.len() > 1)
        .map(|(declared, entries)| format!("`{declared}` in {}", entries.join(", ")))
        .collect();
    if !duplicates.is_empty() {
        bail!("duplicate declared skill names: {}", duplicates.join("; "));
    }
    Ok(())
}
