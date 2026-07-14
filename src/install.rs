use crate::digest::{digest_tree, inspect_tree, EntryKind, TreeRecord, MARKER_FILE};
use crate::lockfile::{LockFile, Mode, SkillEntry};
use crate::scope::Scope;
use crate::source::{validate_skill_tree, SkillMetadata};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub version: u32,
    pub entry: String,
    pub scope: String,
    pub lock: String,
    pub repository: String,
    pub commit: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PathChange {
    pub path: String,
    pub change: String,
}

#[derive(Clone, Debug)]
pub struct EntryState {
    pub state: String,
    pub details: Vec<String>,
    pub metadata: Option<SkillMetadata>,
}

pub fn marker_for(scope: &Scope, name: &str, entry: &SkillEntry) -> Result<Marker> {
    let source = entry
        .source
        .as_ref()
        .context("vendored entry has no source")?;
    let resolved = entry
        .resolved
        .as_ref()
        .context("vendored entry has no resolved data")?;
    Ok(Marker {
        version: 1,
        entry: name.to_owned(),
        scope: scope.label().to_owned(),
        lock: ".agents/skills.lock.yaml".to_owned(),
        repository: source.repository.clone(),
        commit: resolved.commit.clone(),
    })
}

pub fn write_marker(root: &Path, marker: &Marker) -> Result<()> {
    let mut data = serde_yaml_ng::to_string(marker)?;
    if !data.ends_with('\n') {
        data.push('\n');
    }
    fs::write(root.join(MARKER_FILE), data)?;
    Ok(())
}

pub fn read_marker(root: &Path) -> Result<Marker> {
    let path = root.join(MARKER_FILE);
    let bytes =
        fs::read(&path).with_context(|| format!("missing managed marker {}", path.display()))?;
    let marker: Marker = serde_yaml_ng::from_slice(&bytes)
        .with_context(|| format!("invalid managed marker {}", path.display()))?;
    if marker.version != 1 {
        bail!("unsupported managed marker version {}", marker.version);
    }
    Ok(marker)
}

pub fn entry_state(scope: &Scope, name: &str, entry: &SkillEntry) -> EntryState {
    let destination = scope.skills_dir.join(&entry.destination);
    if let Err(error) = validate_destination_location(scope, entry) {
        return EntryState {
            state: "invalid".to_owned(),
            details: vec![format!("{error:#}")],
            metadata: None,
        };
    }
    if !destination.exists() {
        return EntryState {
            state: "missing".to_owned(),
            details: vec![format!("{} does not exist", destination.display())],
            metadata: None,
        };
    }
    let metadata = match validate_skill_tree(&destination) {
        Ok(metadata) => metadata,
        Err(error) => {
            return EntryState {
                state: "invalid".to_owned(),
                details: vec![format!("{error:#}")],
                metadata: None,
            };
        }
    };
    if entry.mode == Mode::Local {
        return EntryState {
            state: "local".to_owned(),
            details: Vec::new(),
            metadata: Some(metadata),
        };
    }
    let expected_marker = match marker_for(scope, name, entry) {
        Ok(marker) => marker,
        Err(error) => {
            return EntryState {
                state: "invalid".to_owned(),
                details: vec![format!("{error:#}")],
                metadata: Some(metadata),
            };
        }
    };
    let marker = match read_marker(&destination) {
        Ok(marker) => marker,
        Err(error) => {
            return EntryState {
                state: "invalid".to_owned(),
                details: vec![format!("{error:#}")],
                metadata: Some(metadata),
            };
        }
    };
    if marker != expected_marker {
        return EntryState {
            state: "invalid".to_owned(),
            details: vec!["managed marker does not match the lock entry".to_owned()],
            metadata: Some(metadata),
        };
    }
    let actual = match digest_tree(&destination) {
        Ok(digest) => digest,
        Err(error) => {
            return EntryState {
                state: "invalid".to_owned(),
                details: vec![format!("{error:#}")],
                metadata: Some(metadata),
            };
        }
    };
    let expected = &entry.resolved.as_ref().expect("validated lock").digest;
    if &actual == expected {
        EntryState {
            state: "clean".to_owned(),
            details: Vec::new(),
            metadata: Some(metadata),
        }
    } else {
        EntryState {
            state: "modified".to_owned(),
            details: vec![format!("digest is {actual}, expected {expected}")],
            metadata: Some(metadata),
        }
    }
}

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

pub fn compare_trees(installed: &Path, expected: &Path) -> Result<Vec<PathChange>> {
    let left = inspect_tree(installed)?;
    let right = inspect_tree(expected)?;
    Ok(compare_records(&left, &right))
}

fn compare_records(
    installed: &BTreeMap<String, TreeRecord>,
    expected: &BTreeMap<String, TreeRecord>,
) -> Vec<PathChange> {
    let paths: BTreeSet<&String> = installed.keys().chain(expected.keys()).collect();
    paths
        .into_iter()
        .filter_map(|path| match (installed.get(path), expected.get(path)) {
            (Some(_), None) => Some(PathChange {
                path: path.clone(),
                change: "added".to_owned(),
            }),
            (None, Some(_)) => Some(PathChange {
                path: path.clone(),
                change: "removed".to_owned(),
            }),
            (Some(left), Some(right)) if left.kind != right.kind => Some(PathChange {
                path: path.clone(),
                change: "type-changed".to_owned(),
            }),
            (Some(left), Some(right))
                if left.kind == EntryKind::File
                    && (left.content_hash != right.content_hash
                        || left.executable != right.executable) =>
            {
                Some(PathChange {
                    path: path.clone(),
                    change: "modified".to_owned(),
                })
            }
            _ => None,
        })
        .collect()
}

pub fn copy_snapshot(source: &Path, parent: &Path) -> Result<PathBuf> {
    fs::create_dir_all(parent)?;
    let temporary = tempfile::Builder::new()
        .prefix(".skillctl-stage-")
        .tempdir_in(parent)?;
    let staged = temporary.keep();
    set_directory_permissions(&staged)?;
    copy_tree(source, &staged)?;
    Ok(staged)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for (relative, record) in inspect_tree(source)? {
        let from = source.join(&relative);
        let to = destination.join(&relative);
        match record.kind {
            EntryKind::Directory => fs::create_dir_all(&to)?,
            EntryKind::File => {
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&from, &to)?;
                set_executable(&to, record.executable)?;
            }
        }
    }
    Ok(())
}

pub fn skill_json(name: &str, entry: &SkillEntry, state: Option<&EntryState>) -> serde_json::Value {
    let source = entry.source.as_ref();
    let resolved = entry.resolved.as_ref();
    json!({
        "name": name,
        "mode": match entry.mode { Mode::Vendored => "vendored", Mode::Local => "local" },
        "destination": entry.destination,
        "source": source.map(|source| json!({
            "repository": source.repository,
            "path": source.path,
            "ref": source.reference,
        })),
        "commit": resolved.map(|resolved| resolved.commit.clone()),
        "digest": resolved.map(|resolved| resolved.digest.clone()),
        "state": state.map(|state| state.state.clone()),
        "details": state.map(|state| state.details.clone()).unwrap_or_default(),
        "declared_name": state.and_then(|state| state.metadata.as_ref()).map(|meta| meta.name.clone()),
    })
}

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(if executable { 0o755 } else { 0o644 });
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _executable: bool) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
