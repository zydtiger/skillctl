use super::support::{load_lock, select_names};
use crate::error::CommandFailure;
use crate::install::{entry_state, skill_json, EntryState};
use crate::lockfile::{Mode, SourceSelector};
use crate::output::Envelope;
use crate::scope::Scope;
use crate::source::validate_skill_tree;
use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;

pub(super) fn check(scope: &Scope, selected: Option<&str>) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let names = select_names(&lock, selected)?;
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    let mut failures = Vec::new();
    let mut declared: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (entry_name, entry) in &lock.skills {
        let destination = scope.skills_dir.join(&entry.destination);
        if let Ok(metadata) = validate_skill_tree(&destination) {
            declared
                .entry(metadata.name)
                .or_default()
                .push(entry_name.clone());
        }
    }
    for name in names {
        let entry = lock.skills.get(&name).expect("selected entry");
        let state = entry_state(scope, &name, entry);
        if !matches!(state.state.as_str(), "clean" | "local") {
            failures.push(format!(
                "`{name}` is {}: {}",
                state.state,
                state.details.join("; ")
            ));
        }
        lines.push(format!("{name}: {}", state.state));
        envelope.skills.push(skill_json(&name, entry, Some(&state)));
    }
    for (declared_name, entries) in declared {
        if entries.len() > 1
            && selected
                .map(|selected| entries.iter().any(|entry| entry == selected))
                .unwrap_or(true)
        {
            failures.push(format!(
                "declared skill name `{declared_name}` is duplicated by {}",
                entries.join(", ")
            ));
        }
    }
    if !failures.is_empty() {
        envelope.ok = false;
        envelope.errors = failures.clone();
        return Err(CommandFailure {
            envelope,
            message: format!("integrity check failed: {}", failures.join("; ")),
        }
        .into());
    }
    Ok((envelope, lines))
}

pub(super) fn status(scope: &Scope) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    let mut states: BTreeMap<String, EntryState> = lock
        .skills
        .iter()
        .map(|(name, entry)| (name.clone(), entry_state(scope, name, entry)))
        .collect();
    let mut declared: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, state) in &states {
        if let Some(metadata) = &state.metadata {
            declared
                .entry(metadata.name.clone())
                .or_default()
                .push(name.clone());
        }
    }
    for (declared_name, entries) in declared {
        if entries.len() > 1 {
            for name in entries {
                let state = states.get_mut(&name).expect("known status entry");
                state.state = "invalid".to_owned();
                state.details.push(format!(
                    "declared skill name `{declared_name}` is used by multiple destinations"
                ));
            }
        }
    }
    for (name, entry) in &lock.skills {
        let state = states.get(name).expect("known status entry");
        let mut record = skill_json(name, entry, Some(state));
        record["update_status"] = json!("unknown");
        envelope.skills.push(record);
        lines.push(format!("{name}: {} (update status unknown)", state.state));
    }
    Ok((envelope, lines))
}

pub(super) fn list(scope: &Scope) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    for (name, entry) in &lock.skills {
        envelope.skills.push(skill_json(name, entry, None));
        let source = entry
            .source
            .as_ref()
            .map(|source| {
                let selector = source
                    .selector()
                    .map(|selector| match selector {
                        SourceSelector::Directory(path) => format!("path={path}"),
                        SourceSelector::File(file) => format!("file={file}"),
                    })
                    .unwrap_or_else(|_| "invalid-selector".to_owned());
                format!("{}:{selector}@{}", source.repository, source.reference)
            })
            .unwrap_or_else(|| "-".to_owned());
        let commit = entry
            .resolved
            .as_ref()
            .map(|resolved| resolved.commit.as_str())
            .unwrap_or("-");
        lines.push(format!(
            "{name}\t{}\t{}\t{source}\t{commit}",
            match entry.mode {
                Mode::Vendored => "vendored",
                Mode::Local => "local",
            },
            entry.destination
        ));
    }
    Ok((envelope, lines))
}
