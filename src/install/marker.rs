use crate::digest::MARKER_FILE;
use crate::lockfile::{safe_source_file, SkillEntry, SourceSelector};
use crate::scope::Scope;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub version: u32,
    pub entry: String,
    pub scope: String,
    pub lock: String,
    pub repository: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub commit: String,
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
    let selector = source.selector()?;
    let file = match selector {
        SourceSelector::Directory(_) => None,
        SourceSelector::File(file) => Some(file),
    };
    Ok(Marker {
        version: if file.is_some() { 2 } else { 1 },
        entry: name.to_owned(),
        scope: scope.label().to_owned(),
        lock: ".agents/skills.lock.yaml".to_owned(),
        repository: source.repository.clone(),
        file,
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
    match (marker.version, marker.file.as_deref()) {
        (1, None) => {}
        (2, Some(file)) => {
            safe_source_file(file).context("invalid managed marker source file")?;
        }
        (1, Some(_)) => bail!("managed marker version 1 must not contain file"),
        (2, None) => bail!("managed marker version 2 requires file"),
        (version, _) => bail!("unsupported managed marker version {version}"),
    }
    Ok(marker)
}
