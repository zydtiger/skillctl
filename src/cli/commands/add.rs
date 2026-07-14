use super::support::{entry_for_destination, install_prepared, load_lock};
use crate::install::{skill_json, validate_declared_names, validate_destination_location};
use crate::lockfile::{
    validate_identifier, Mode, Resolved, SkillEntry, SourceSelector, SourceSpec,
};
use crate::output::Envelope;
use crate::scope::Scope;
use crate::source::{acquire, validate_skill_tree};
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::fs;

pub(super) fn run(
    scope: &Scope,
    repository: &str,
    selector: &SourceSelector,
    requested_name: Option<&str>,
    reference: Option<&str>,
    dry_run: bool,
) -> Result<(Envelope, Vec<String>)> {
    let mut lock = load_lock(scope)?;
    validate_declared_names(scope, &lock)?;
    if matches!(selector, SourceSelector::File(_)) && requested_name.is_none() {
        bail!("--name is required with --file");
    }
    let upgrades_lock = matches!(selector, SourceSelector::File(_)) && lock.version == 1;
    let repository = normalize_repository(repository)?;
    let snapshot = acquire(&repository, selector, reference)?;
    let name = requested_name.unwrap_or(&snapshot.metadata.name);
    validate_identifier(name).context("invalid --name")?;
    if lock.skills.contains_key(name) {
        bail!("lock entry `{name}` already exists");
    }
    if lock.skills.values().any(|entry| entry.destination == name) {
        bail!("destination `{name}` already appears in the lock");
    }
    let destination = scope.skills_dir.join(name);
    validate_destination_location(scope, &entry_for_destination(name))?;
    if destination.exists() {
        bail!(
            "destination {} already exists and is unmanaged",
            destination.display()
        );
    }
    for (existing_name, existing) in &lock.skills {
        let existing_path = scope.skills_dir.join(&existing.destination);
        if existing_path.exists() {
            let metadata = validate_skill_tree(&existing_path)?;
            if metadata.name == snapshot.metadata.name {
                bail!(
                    "declared skill name `{}` collides with lock entry `{existing_name}`",
                    snapshot.metadata.name
                );
            }
        }
    }
    let entry = SkillEntry {
        mode: Mode::Vendored,
        source: Some(SourceSpec {
            repository: repository.clone(),
            path: match selector {
                SourceSelector::Directory(path) => Some(path.clone()),
                SourceSelector::File(_) => None,
            },
            file: match selector {
                SourceSelector::Directory(_) => None,
                SourceSelector::File(file) => Some(file.clone()),
            },
            reference: snapshot.reference.clone(),
        }),
        resolved: Some(Resolved {
            commit: snapshot.commit.clone(),
            digest: snapshot.digest.clone(),
        }),
        destination: name.to_owned(),
    };
    if upgrades_lock {
        lock.version = 2;
    }
    lock.skills.insert(name.to_owned(), entry.clone());
    lock.validate()?;
    let mut envelope = Envelope::new(scope);
    envelope.skills.push(skill_json(name, &entry, None));
    envelope.changes.push(json!({
        "action": "add",
        "name": name,
        "destination": entry.destination,
        "source_kind": match selector {
            SourceSelector::Directory(_) => "path",
            SourceSelector::File(_) => "file",
        },
        "source": match selector {
            SourceSelector::Directory(path) | SourceSelector::File(path) => path,
        },
        "commit": snapshot.commit,
        "digest": snapshot.digest,
        "lock_version": lock.version,
        "dry_run": dry_run,
    }));
    if !dry_run {
        install_prepared(scope, name, &entry, &snapshot, &lock)?;
    }
    Ok((
        envelope,
        vec![format!(
            "{} `{name}` at {} ({}){}",
            if dry_run { "would add" } else { "added" },
            snapshot.commit,
            snapshot.digest,
            if upgrades_lock {
                " and upgrade lock schema from v1 to v2"
            } else {
                ""
            }
        )],
    ))
}

fn normalize_repository(repository: &str) -> Result<String> {
    let path = std::path::Path::new(repository);
    if path.exists() {
        Ok(fs::canonicalize(path)?
            .to_str()
            .context("local repository path is not valid UTF-8")?
            .to_owned())
    } else {
        Ok(repository.to_owned())
    }
}
