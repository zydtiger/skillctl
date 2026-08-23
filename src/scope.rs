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

        // The global installation lives at `${SKILLCTL_HOME:-$HOME}/.agents` and
        // is structurally identical to a project lock rooted at the home
        // directory. Skipping that one root keeps the upward walk from silently
        // adopting the global installation as a project, which no verb may do.
        let global = Self::global().ok();
        let start = absolute(cwd)?;
        for directory in start.ancestors() {
            if global
                .as_ref()
                .is_some_and(|global| same_directory(directory, &global.root))
            {
                continue;
            }
            if directory.join(LOCK_RELATIVE).is_file() {
                return Ok(Self::at(ScopeKind::Project, directory.to_path_buf()));
            }
        }
        let redirect = global
            .filter(|global| global.lock_file.is_file())
            .map(|global| {
                format!(
                    "; the global installation exists at {}; use --global to operate on it",
                    global.lock_file.display()
                )
            })
            .unwrap_or_default();
        bail!(
            "no project {} found from {} upward; run `skillctl init` in the intended project root{}",
            LOCK_RELATIVE,
            start.display(),
            redirect
        )
    }

    fn global() -> Result<Self> {
        let home = env::var_os("SKILLCTL_HOME")
            .or_else(|| env::var_os("HOME"))
            .context("neither SKILLCTL_HOME nor HOME is set")?;
        Ok(Self::at(ScopeKind::Global, PathBuf::from(home)))
    }
}

/// Compare two directories by path, falling back to canonical paths so a
/// symlinked home (`/var` on macOS, or a linked `$HOME`) still matches the
/// resolved working directory the ancestor walk starts from.
fn same_directory(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}
