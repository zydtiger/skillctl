use super::validate::{
    safe_relative_path, safe_source_file, safe_source_path, validate_commit, validate_digest,
    validate_git_reference, validate_identifier, validate_repository,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
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
            version: 2,
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
        if !matches!(self.version, 1 | 2) {
            bail!(
                "unsupported lock schema version {}; supported: 1 and 2",
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
                    match (source.path.as_deref(), source.file.as_deref()) {
                        (Some(path), None) => {
                            safe_source_path(path)
                                .with_context(|| format!("invalid source path for `{name}`"))?;
                        }
                        (None, Some(file)) if self.version == 2 => {
                            safe_source_file(file)
                                .with_context(|| format!("invalid source file for `{name}`"))?;
                        }
                        (None, Some(_)) => {
                            bail!("source.file for `{name}` requires lock schema version 2");
                        }
                        (Some(_), Some(_)) => {
                            bail!("vendored entry `{name}` must contain only one of source.path or source.file");
                        }
                        (None, None) => {
                            bail!("vendored entry `{name}` requires source.path or source.file");
                        }
                    }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceSelector {
    Directory(String),
    File(String),
}

impl SourceSpec {
    pub fn selector(&self) -> Result<SourceSelector> {
        match (&self.path, &self.file) {
            (Some(path), None) => Ok(SourceSelector::Directory(path.clone())),
            (None, Some(file)) => Ok(SourceSelector::File(file.clone())),
            (Some(_), Some(_)) => bail!("source contains both path and file"),
            (None, None) => bail!("source contains neither path nor file"),
        }
    }
}
