use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockFile {
    pub version: u32,
    #[serde(default)]
    pub skills: BTreeMap<String, SkillEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Vendored,
    Local,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillEntry {
    pub mode: Mode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<Resolved>,
    pub destination: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    pub repository: String,
    pub path: String,
    #[serde(rename = "ref")]
    pub reference: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Resolved {
    pub commit: String,
    pub digest: String,
}

impl LockFile {
    pub fn empty() -> Self {
        Self {
            version: 1,
            skills: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)
            .with_context(|| format!("could not read lock file {}", path.display()))?;
        let lock: Self = serde_yaml_ng::from_slice(&bytes)
            .with_context(|| format!("invalid lock file {}", path.display()))?;
        lock.validate()?;
        Ok(lock)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!(
                "unsupported lock schema version {}; supported: 1",
                self.version
            );
        }
        let mut destinations = BTreeSet::new();
        for (name, entry) in &self.skills {
            validate_identifier(name).with_context(|| format!("invalid lock entry `{name}`"))?;
            safe_relative_path(&entry.destination)
                .with_context(|| format!("invalid destination for `{name}`"))?;
            if !destinations.insert(entry.destination.clone()) {
                bail!("duplicate destination `{}`", entry.destination);
            }
            match entry.mode {
                Mode::Local => {
                    if entry.source.is_some() || entry.resolved.is_some() {
                        bail!("local entry `{name}` must not contain source or resolved fields");
                    }
                }
                Mode::Vendored => {
                    let source = entry
                        .source
                        .as_ref()
                        .with_context(|| format!("vendored entry `{name}` requires source"))?;
                    let resolved = entry
                        .resolved
                        .as_ref()
                        .with_context(|| format!("vendored entry `{name}` requires resolved"))?;
                    if source.repository.trim().is_empty() || source.reference.trim().is_empty() {
                        bail!("vendored entry `{name}` has an empty repository or ref");
                    }
                    validate_repository(&source.repository)
                        .with_context(|| format!("invalid source repository for `{name}`"))?;
                    validate_git_reference(&source.reference)
                        .with_context(|| format!("invalid source ref for `{name}`"))?;
                    safe_source_path(&source.path)
                        .with_context(|| format!("invalid source path for `{name}`"))?;
                    validate_commit(&resolved.commit)
                        .with_context(|| format!("invalid resolved commit for `{name}`"))?;
                    validate_digest(&resolved.digest)
                        .with_context(|| format!("invalid resolved digest for `{name}`"))?;
                }
            }
        }
        let paths: Vec<(&String, PathBuf)> = self
            .skills
            .iter()
            .map(|(name, entry)| (name, PathBuf::from(&entry.destination)))
            .collect();
        for (index, (left_name, left)) in paths.iter().enumerate() {
            for (right_name, right) in paths.iter().skip(index + 1) {
                if left.starts_with(right) || right.starts_with(left) {
                    bail!(
                        "overlapping destinations for `{left_name}` and `{right_name}` are not allowed"
                    );
                }
            }
        }
        Ok(())
    }

    pub fn yaml(&self) -> Result<Vec<u8>> {
        let mut text = serde_yaml_ng::to_string(self)?;
        if !text.ends_with('\n') {
            text.push('\n');
        }
        Ok(text.into_bytes())
    }
}

pub fn safe_source_path(value: &str) -> Result<PathBuf> {
    if value == "." {
        Ok(PathBuf::new())
    } else {
        safe_relative_path(value)
    }
}

pub fn safe_relative_path(value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.contains('\0') || value.contains('\\') {
        bail!("path must be a nonempty POSIX-style relative path");
    }
    let path = Path::new(value);
    if path.is_absolute() {
        bail!("absolute paths are not allowed");
    }
    let mut count = 0;
    for component in path.components() {
        match component {
            Component::Normal(part) if !part.is_empty() => {
                let part = part.to_str().context("path is not valid UTF-8")?;
                if part.chars().any(char::is_control) {
                    bail!("control characters are not allowed in paths");
                }
                count += 1;
            }
            _ => bail!("dot, empty, root, prefix, and parent path components are not allowed"),
        }
    }
    if count == 0 || value.split('/').any(str::is_empty) {
        bail!("empty path components are not allowed");
    }
    Ok(path.to_path_buf())
}

pub fn validate_identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('-')
        || value.ends_with('-')
        || value.split('-').any(str::is_empty)
        || !value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        bail!("must be 1-64 lowercase letters, digits, or hyphens without edge hyphens");
    }
    Ok(())
}

pub fn validate_repository(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') || value.chars().any(char::is_control) {
        bail!("repository must be nonempty and contain no control characters");
    }
    Ok(())
}

pub fn validate_git_reference(value: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.starts_with('-')
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        bail!("ref must be nonempty, must not start with '-', and contain no control characters");
    }
    Ok(())
}

fn validate_commit(value: &str) -> Result<()> {
    if value.len() != 40 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("must be a 40-character hexadecimal Git commit");
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        bail!("must start with sha256:");
    };
    if hex.len() != 64 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("must contain a 64-character hexadecimal SHA-256");
    }
    Ok(())
}
