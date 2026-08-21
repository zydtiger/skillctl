use anyhow::{bail, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use walkdir::WalkDir;

pub const MARKER_FILE: &str = ".skillctl-managed";

/// Directories an interpreter writes beside a skill's own files when a
/// consumer runs one of its scripts. They are machine-generated caches rather
/// than authored content, so a skill that has only been used would otherwise
/// read as modified and block its own update.
pub const IGNORED_DIRECTORIES: [&str; 1] = ["__pycache__"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Directory,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRecord {
    pub kind: EntryKind,
    pub executable: bool,
    pub size: u64,
    pub content_hash: Option<String>,
}

pub fn digest_tree(root: &Path) -> Result<String> {
    let records = inspect_tree(root)?;
    let mut hasher = Sha256::new();
    hasher.update(b"skillctl-tree-v1\0");
    for (path, record) in records {
        frame(&mut hasher, path.as_bytes());
        match record.kind {
            EntryKind::Directory => hasher.update(b"D"),
            EntryKind::File => {
                hasher.update(b"F");
                hasher.update([u8::from(record.executable)]);
                hasher.update(record.size.to_be_bytes());
                let mut file = fs::File::open(root.join(&path))?;
                let mut buffer = [0_u8; 64 * 1024];
                loop {
                    let read = file.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..read]);
                }
            }
        }
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

pub fn inspect_tree(root: &Path) -> Result<BTreeMap<String, TreeRecord>> {
    let root_metadata = fs::symlink_metadata(root)
        .with_context(|| format!("could not inspect skill tree {}", root.display()))?;
    if root_metadata.file_type().is_symlink() {
        bail!("skill tree root must not be a symlink: {}", root.display());
    }
    if !root_metadata.is_dir() {
        bail!("skill tree {} is not a directory", root.display());
    }
    let mut records = BTreeMap::new();
    for item in WalkDir::new(root)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| !is_ignored_directory(entry))
    {
        let item = item.with_context(|| format!("could not walk {}", root.display()))?;
        let relative = item.path().strip_prefix(root)?;
        let path = relative
            .to_str()
            .context("skill tree contains a non-UTF-8 path")?
            .replace(std::path::MAIN_SEPARATOR, "/");
        let metadata = fs::symlink_metadata(item.path())?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            bail!("symlink is not allowed: {path}");
        }
        if path == MARKER_FILE {
            if !file_type.is_file() {
                bail!("managed marker must be an ordinary file");
            }
            continue;
        }
        let record = if file_type.is_dir() {
            TreeRecord {
                kind: EntryKind::Directory,
                executable: false,
                size: 0,
                content_hash: None,
            }
        } else if file_type.is_file() {
            let bytes = fs::read(item.path())?;
            TreeRecord {
                kind: EntryKind::File,
                executable: executable(&metadata),
                size: metadata.len(),
                content_hash: Some(hex::encode(Sha256::digest(bytes))),
            }
        } else {
            bail!("special file is not allowed: {path}");
        };
        records.insert(path, record);
    }
    Ok(records)
}

/// A symlink is deliberately not matched here: it keeps its own rejection
/// rather than being skipped by name.
fn is_ignored_directory(entry: &walkdir::DirEntry) -> bool {
    entry.depth() > 0
        && entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| IGNORED_DIRECTORIES.contains(&name))
}

fn frame(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(unix)]
fn executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn digest_is_deterministic_and_ignores_marker() {
        let temp = tempdir().unwrap();
        fs::create_dir(temp.path().join("nested")).unwrap();
        fs::write(temp.path().join("nested/file"), b"hello").unwrap();
        let first = digest_tree(temp.path()).unwrap();
        fs::write(temp.path().join(MARKER_FILE), b"metadata").unwrap();
        assert_eq!(first, digest_tree(temp.path()).unwrap());

        let other = tempdir().unwrap();
        fs::create_dir(other.path().join("nested")).unwrap();
        fs::write(other.path().join("nested/file"), b"hello").unwrap();
        assert_eq!(first, digest_tree(other.path()).unwrap());
    }

    #[test]
    fn digest_ignores_generated_bytecode_caches() {
        let temp = tempdir().unwrap();
        fs::create_dir(temp.path().join("scripts")).unwrap();
        fs::write(temp.path().join("scripts/helper.py"), b"x = 1\n").unwrap();
        let before = digest_tree(temp.path()).unwrap();

        let cache = temp.path().join("scripts/__pycache__");
        fs::create_dir(&cache).unwrap();
        fs::write(cache.join("helper.cpython-312.pyc"), b"compiled").unwrap();
        assert_eq!(before, digest_tree(temp.path()).unwrap());

        let records = inspect_tree(temp.path()).unwrap();
        assert!(records.keys().all(|path| !path.contains("__pycache__")));
    }
}
