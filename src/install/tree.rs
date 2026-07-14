use crate::digest::{inspect_tree, EntryKind, TreeRecord};
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct PathChange {
    pub path: String,
    pub change: String,
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
