use super::support::{load_lock, select_names};
use crate::error::CommandFailure;
use crate::install::{entry_state, skill_json, EntryState};
use crate::lockfile::{LockFile, Mode, SourceSelector};
use crate::output::Envelope;
use crate::scope::{Scope, ScopeKind};
use crate::source::{acquire_many, validate_skill_tree, AcquiredRepository};
use anyhow::Result;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
struct UpstreamState {
    status: &'static str,
    commit: Option<String>,
    content_changed: Option<bool>,
    details: Vec<String>,
}

#[derive(Debug)]
struct StatusRow {
    name: String,
    local: String,
    upstream: String,
    pin: String,
    reference: String,
    action: String,
}

pub(super) fn check(scope: &Scope, selected: &[String]) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let names = select_names(&lock, selected)?;
    let mut envelope = Envelope::new(scope);
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
    for name in &names {
        let entry = lock.skills.get(name).expect("selected entry");
        let state = entry_state(scope, name, entry);
        if !matches!(state.state.as_str(), "clean" | "local") {
            failures.push(format!(
                "`{name}` is {}: {}",
                state.state,
                state.details.join("; ")
            ));
        }
        envelope.skills.push(skill_json(name, entry, Some(&state)));
    }
    for (declared_name, entries) in declared {
        if entries.len() > 1
            && (selected.is_empty() || entries.iter().any(|entry| selected.contains(entry)))
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
    Ok((
        envelope,
        vec![format!(
            "OK: {} lock {} passed offline integrity checks",
            names.len(),
            if names.len() == 1 { "entry" } else { "entries" }
        )],
    ))
}

pub(super) fn status(scope: &Scope, offline: bool) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let states = local_states(scope, &lock);
    let upstream = upstream_states(&lock, offline);
    let mut envelope = Envelope::new(scope);
    let mut rows = Vec::new();
    let mut source_errors = BTreeSet::new();
    let ctl = scope_ctl(scope);
    // Names whose recommended action is the plain `update NAME` command,
    // including its `(pin only)` form: both are satisfied by the exact same
    // invocation, unlike the `review: ...; then ... --force` variants, which
    // need deliberate handling and stay excluded. Collected in table order,
    // so a stale subset spanning several entries can be pointed at one
    // combined invocation below the table.
    let mut update_candidates = Vec::new();

    for (name, entry) in &lock.skills {
        let local = states.get(name).expect("known local status entry");
        let remote = upstream.get(name).expect("known upstream status entry");
        let action = recommended_action(scope, name, &local.state, remote.status);
        if action == format!("{ctl} update {name}")
            || action == format!("{ctl} update {name} (pin only)")
        {
            update_candidates.push(name.clone());
        }
        let resolved = entry.resolved.as_ref();
        let mut record = skill_json(name, entry, Some(local));
        record["local_status"] = json!(local.state);
        record["upstream_status"] = json!(remote.status);
        record["update_status"] = json!(remote.status);
        record["pinned_commit"] = json!(resolved.map(|value| value.commit.clone()));
        record["upstream_commit"] = json!(remote.commit);
        record["content_changed"] = json!(remote.content_changed);
        record["upstream_details"] = json!(remote.details);
        record["recommended_action"] = json!(action);
        envelope.skills.push(record);

        if !remote.details.is_empty() {
            let source = entry.source.as_ref().expect("vendored source");
            source_errors.insert(format!(
                "{}@{}: {}",
                source.repository,
                source.reference,
                remote.details.join("; ")
            ));
        }
        rows.push(StatusRow {
            name: name.clone(),
            local: local.state.clone(),
            upstream: match remote.status {
                "not_checked" | "not_applicable" => "-".to_owned(),
                value => value.to_owned(),
            },
            pin: resolved
                .map(|value| abbreviated_commit(&value.commit))
                .unwrap_or_else(|| "-".to_owned()),
            reference: entry
                .source
                .as_ref()
                .map(|source| source.reference.clone())
                .unwrap_or_else(|| "-".to_owned()),
            action: match (action.as_str(), remote.status, local.state.as_str()) {
                ("none", _, _) | (_, "not_checked", "clean") => "-".to_owned(),
                _ => action,
            },
        });
    }

    let mut lines = render_status_table(&rows);
    if update_candidates.len() > 1 {
        lines.push(format!(
            "run all: {ctl} update {}",
            update_candidates.join(" ")
        ));
    }
    if offline
        && lock
            .skills
            .values()
            .any(|entry| entry.mode == Mode::Vendored)
    {
        lines.push("Upstream checks were skipped by --offline.".to_owned());
    }
    for error in source_errors {
        lines.push(format!("Upstream error: {error}"));
    }
    Ok((envelope, lines))
}

fn local_states(scope: &Scope, lock: &LockFile) -> BTreeMap<String, EntryState> {
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
    states
}

fn upstream_states(lock: &LockFile, offline: bool) -> BTreeMap<String, UpstreamState> {
    let mut states = BTreeMap::new();
    let mut groups: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (name, entry) in &lock.skills {
        if entry.mode == Mode::Local {
            states.insert(
                name.clone(),
                UpstreamState {
                    status: "not_applicable",
                    commit: None,
                    content_changed: None,
                    details: Vec::new(),
                },
            );
        } else if offline {
            states.insert(
                name.clone(),
                UpstreamState {
                    status: "not_checked",
                    commit: None,
                    content_changed: None,
                    details: Vec::new(),
                },
            );
        } else {
            let source = entry.source.as_ref().expect("validated vendored source");
            groups
                .entry((source.repository.clone(), source.reference.clone()))
                .or_default()
                .push(name.clone());
        }
    }

    // Acquire every distinct group concurrently, bounded inside
    // `acquire_many`, so several unreachable or slow sources do not serialize
    // the network phase. Results are collected before any per-name state is
    // derived, so the derivation below runs sequentially over an already
    // completed, ordered map and produces the same output regardless of
    // completion order.
    let keys: Vec<(String, String)> = groups.keys().cloned().collect();
    let results = acquire_many(&keys);
    let mut acquired_groups: BTreeMap<(String, String), Result<AcquiredRepository>> =
        keys.into_iter().zip(results).collect();

    for ((repository, reference), names) in groups {
        let result = acquired_groups
            .remove(&(repository.clone(), reference.clone()))
            .expect("every built group was acquired");
        match result {
            Ok(acquired) => {
                for name in names {
                    let entry = lock.skills.get(&name).expect("grouped lock entry");
                    let source = entry.source.as_ref().expect("vendored source");
                    let resolved = entry.resolved.as_ref().expect("vendored pin");
                    let result = source
                        .selector()
                        .and_then(|selector| acquired.select(&selector));
                    let state = match result {
                        Ok(snapshot) => {
                            let content_changed = snapshot.digest != resolved.digest;
                            let status = if content_changed {
                                "update_available"
                            } else if snapshot.commit != resolved.commit {
                                "source_advanced"
                            } else {
                                "current"
                            };
                            UpstreamState {
                                status,
                                commit: Some(snapshot.commit),
                                content_changed: Some(content_changed),
                                details: Vec::new(),
                            }
                        }
                        Err(error) => UpstreamState {
                            status: "invalid",
                            commit: Some(acquired.commit.clone()),
                            content_changed: None,
                            details: vec![format!("{error:#}")],
                        },
                    };
                    states.insert(name, state);
                }
            }
            Err(error) => {
                let detail = format!("{error:#}");
                for name in names {
                    states.insert(
                        name,
                        UpstreamState {
                            status: "unreachable",
                            commit: None,
                            content_changed: None,
                            details: vec![detail.clone()],
                        },
                    );
                }
            }
        }
    }
    states
}

/// Scope-qualified command prefix suggested commands are built from. It must
/// carry the invoked scope, or copying a suggestion from a `--global` run
/// would act on the project lock instead.
fn scope_ctl(scope: &Scope) -> &'static str {
    match scope.kind {
        ScopeKind::Project => "skillctl",
        ScopeKind::Global => "skillctl --global",
    }
}

fn recommended_action(scope: &Scope, name: &str, local: &str, upstream: &str) -> String {
    if local == "local" {
        return "project-owned; manage locally".to_owned();
    }
    let ctl = scope_ctl(scope);
    let upstream_action = || match upstream {
        "update_available" => Some(format!("{ctl} update {name}")),
        "source_advanced" => Some(format!("{ctl} update {name} (pin only)")),
        "unreachable" => Some("retry; verify repository/ref access".to_owned()),
        "invalid" => Some("fix upstream selected skill".to_owned()),
        "not_checked" => Some(format!("run {ctl} status online")),
        _ => None,
    };
    match (local, upstream) {
        ("missing", "update_available" | "source_advanced") => format!("{ctl} update {name}"),
        ("modified", "update_available" | "source_advanced") => {
            format!("review: {ctl} diff {name}; then {ctl} update {name} --force")
        }
        ("invalid", "update_available" | "source_advanced") => {
            format!("review, then {ctl} update {name} --force")
        }
        ("missing", "unreachable") => {
            format!("verify repository/ref access; then {ctl} sync {name}")
        }
        ("modified", "unreachable") => {
            format!("review: {ctl} diff {name}; verify repository/ref access")
        }
        ("invalid", "unreachable") => {
            format!("verify repository/ref access; review, then {ctl} sync {name} --force")
        }
        ("missing", "invalid") => format!("fix upstream selected skill; then {ctl} sync {name}"),
        ("modified", "invalid") => {
            format!("review: {ctl} diff {name}; fix upstream selected skill")
        }
        ("invalid", "invalid") => "fix local and upstream skill structure".to_owned(),
        ("missing", _) => format!("{ctl} sync {name}"),
        ("modified", _) => format!("review: {ctl} diff {name}"),
        ("invalid", _) => format!("review, then {ctl} sync {name} --force"),
        _ => upstream_action().unwrap_or_else(|| "none".to_owned()),
    }
}

fn abbreviated_commit(commit: &str) -> String {
    commit.chars().take(12).collect()
}

fn render_status_table(rows: &[StatusRow]) -> Vec<String> {
    if rows.is_empty() {
        return vec!["No lock entries.".to_owned()];
    }
    let headers = ["NAME", "LOCAL", "UPSTREAM", "PIN", "REF", "ACTION"];
    let mut widths = headers.map(str::len);
    for row in rows {
        for (index, value) in [
            row.name.as_str(),
            row.local.as_str(),
            row.upstream.as_str(),
            row.pin.as_str(),
            row.reference.as_str(),
            row.action.as_str(),
        ]
        .iter()
        .enumerate()
        {
            widths[index] = widths[index].max(value.len());
        }
    }
    let format_row = |values: [&str; 6]| {
        format!(
            "{:<name_width$}  {:<local_width$}  {:<upstream_width$}  {:<pin_width$}  {:<ref_width$}  {}",
            values[0],
            values[1],
            values[2],
            values[3],
            values[4],
            values[5],
            name_width = widths[0],
            local_width = widths[1],
            upstream_width = widths[2],
            pin_width = widths[3],
            ref_width = widths[4],
        )
    };
    let mut lines = vec![format_row(headers)];
    lines.extend(rows.iter().map(|row| {
        format_row([
            &row.name,
            &row.local,
            &row.upstream,
            &row.pin,
            &row.reference,
            &row.action,
        ])
    }));
    lines
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
