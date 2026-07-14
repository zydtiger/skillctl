use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::env;
use std::path::{Path, PathBuf};

pub const LOCK_RELATIVE: &str = ".agents/skills.lock.yaml";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    Project,
    Global,
}

#[derive(Clone, Debug)]
pub struct Scope {
    pub kind: ScopeKind,
    pub root: PathBuf,
    pub agents_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub lock_file: PathBuf,
}

impl Scope {
    fn at(kind: ScopeKind, root: PathBuf) -> Self {
        let agents_dir = match kind {
            ScopeKind::Project => root.join(".agents"),
            ScopeKind::Global => root.join(".agents"),
        };
        Self {
            kind,
            skills_dir: agents_dir.join("skills"),
            lock_file: agents_dir.join("skills.lock.yaml"),
            agents_dir,
            root,
        }
    }

    pub fn label(&self) -> &'static str {
        match self.kind {
            ScopeKind::Project => "project",
            ScopeKind::Global => "global",
        }
    }

    pub fn for_init(global: bool, cwd: &Path) -> Result<Self> {
        if global {
            Self::global()
        } else {
            Ok(Self::at(ScopeKind::Project, absolute(cwd)?))
        }
    }

    pub fn discover(global: bool, cwd: &Path) -> Result<Self> {
        if global {
            let scope = Self::global()?;
            if !scope.lock_file.is_file() {
                bail!(
                    "global lock not found at {}; run `skillctl --global init`",
                    scope.lock_file.display()
                );
            }
            return Ok(scope);
        }

        let start = absolute(cwd)?;
        for directory in start.ancestors() {
            if directory.join(LOCK_RELATIVE).is_file() {
                return Ok(Self::at(ScopeKind::Project, directory.to_path_buf()));
            }
        }
        bail!(
            "no {} found from {} upward; run `skillctl init` in the intended project root",
            LOCK_RELATIVE,
            start.display()
        )
    }

    fn global() -> Result<Self> {
        let home = env::var_os("SKILLCTL_HOME")
            .or_else(|| env::var_os("HOME"))
            .context("neither SKILLCTL_HOME nor HOME is set")?;
        Ok(Self::at(ScopeKind::Global, PathBuf::from(home)))
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}
