mod git;
mod skill;

pub use skill::{validate_skill_tree, SkillMetadata};

use crate::digest::digest_tree;
use crate::lockfile::{
    safe_source_file, safe_source_path, validate_git_reference, validate_repository, SourceSelector,
};
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[derive(Debug)]
pub struct Snapshot {
    _temp: TempDir,
    pub root: PathBuf,
    pub commit: String,
    pub reference: String,
    pub digest: String,
    pub metadata: SkillMetadata,
}

#[derive(Debug)]
pub struct AcquiredRepository {
    _temp: TempDir,
    bare: PathBuf,
    pub commit: String,
    pub reference: String,
}

pub fn acquire_repository(repository: &str, reference: Option<&str>) -> Result<AcquiredRepository> {
    validate_repository(repository)?;
    let temp = tempfile::tempdir().context("could not create source staging directory")?;
    let bare = temp.path().join("repository.git");

    // A user-supplied reference is validated before it ever reaches a Git
    // argument, since it drives the `--branch` and `fetch` attempts below.
    let requested = match reference {
        Some(value) if !value.trim().is_empty() => {
            validate_git_reference(value)?;
            Some(value.to_owned())
        }
        Some(_) => bail!("ref must not be empty"),
        None => None,
    };
    git::acquire_bare(repository, &bare, requested.as_deref())?;

    let followed = match requested {
        Some(value) => value,
        None => {
            let discovered = git::default_branch(&bare)?;
            validate_git_reference(&discovered)?;
            discovered
        }
    };
    let commit = git::resolve_revision(&bare, &followed)?;
    Ok(AcquiredRepository {
        _temp: temp,
        bare,
        commit,
        reference: followed,
    })
}

impl AcquiredRepository {
    pub fn select(&self, selector: &SourceSelector) -> Result<Snapshot> {
        match selector {
            SourceSelector::Directory(path) => {
                safe_source_path(path)?;
            }
            SourceSelector::File(file) => {
                safe_source_file(file)?;
            }
        }
        let temp = tempfile::tempdir().context("could not create source snapshot directory")?;
        let root = temp.path().join("snapshot");
        fs::create_dir(&root)?;
        match selector {
            SourceSelector::Directory(path) => {
                let path = safe_source_path(path)?;
                git::export_tree(&self.bare, &self.commit, &path, &root)?;
            }
            SourceSelector::File(file) => {
                let file = safe_source_file(file)?;
                git::export_file(&self.bare, &self.commit, &file, &root)?;
            }
        }
        if root.join(crate::digest::MARKER_FILE).exists() {
            bail!(
                "source tree must not contain reserved file {}",
                crate::digest::MARKER_FILE
            );
        }
        let metadata = validate_skill_tree(&root)?;
        let digest = digest_tree(&root)?;
        Ok(Snapshot {
            _temp: temp,
            root,
            commit: self.commit.clone(),
            reference: self.reference.clone(),
            digest,
            metadata,
        })
    }
}

pub fn acquire(
    repository: &str,
    selector: &SourceSelector,
    reference: Option<&str>,
) -> Result<Snapshot> {
    acquire_repository(repository, reference)?.select(selector)
}
