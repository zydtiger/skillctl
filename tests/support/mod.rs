#![allow(dead_code)]

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

pub(crate) struct Fixture {
    pub(crate) _temp: TempDir,
    pub(crate) repo: PathBuf,
    pub(crate) project: PathBuf,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("source");
        let project = temp.path().join("project");
        fs::create_dir_all(repo.join("skills/demo")).unwrap();
        fs::create_dir_all(&project).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "tests@example.com"]);
        git(&repo, &["config", "user.name", "Tests"]);
        write_skill(&repo.join("skills/demo"), "demo", "one");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "initial"]);
        Self {
            _temp: temp,
            repo,
            project,
        }
    }

    pub(crate) fn command(&self) -> Command {
        command_in(&self.project)
    }

    pub(crate) fn init_add(&self) {
        assert_ok(self.command().arg("init").output().unwrap());
        self.init_add_after_existing_init();
    }

    pub(crate) fn init_add_after_existing_init(&self) {
        assert_ok(
            self.command()
                .args(["add", self.repo.to_str().unwrap(), "--path", "skills/demo"])
                .output()
                .unwrap(),
        );
    }

    pub(crate) fn advance(&self, content: &str) -> String {
        fs::write(self.repo.join("skills/demo/data.txt"), content).unwrap();
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "-qm", "advance"]);
        git_stdout(&self.repo, &["rev-parse", "HEAD"])
    }

    /// Advance the source ref without changing any vendored skill's content.
    pub(crate) fn advance_unrelated(&self, content: &str) -> String {
        fs::write(self.repo.join("README.md"), content).unwrap();
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "-qm", "advance unrelated"]);
        git_stdout(&self.repo, &["rev-parse", "HEAD"])
    }
}

pub(crate) fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_skillctl")
}

pub(crate) fn command_in(directory: &Path) -> Command {
    let mut command = Command::new(binary());
    command.current_dir(directory);
    command
}

pub(crate) fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert_ok(output);
}

pub(crate) fn git_stdout(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert_ok_ref(&output);
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub(crate) fn assert_ok(output: Output) {
    assert_ok_ref(&output);
}

pub(crate) fn assert_ok_ref(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn assert_fail(output: &Output) {
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

pub(crate) fn write_skill(directory: &Path, name: &str, data: &str) {
    fs::create_dir_all(directory).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test skill {name}.\n---\n\n# Test\n"),
    )
    .unwrap();
    fs::write(directory.join("data.txt"), data).unwrap();
}

/// Content plus inode, so a byte-identical reinstall is still detected.
#[cfg(unix)]
pub(crate) fn file_identity(path: &Path) -> (String, u64) {
    use std::os::unix::fs::MetadataExt;
    let content = fs::read_to_string(path).unwrap();
    (content, fs::metadata(path).unwrap().ino())
}

#[cfg(not(unix))]
pub(crate) fn file_identity(path: &Path) -> (String, u64) {
    (fs::read_to_string(path).unwrap(), 0)
}

pub(crate) fn read_lock(fixture: &Fixture) -> String {
    fs::read_to_string(fixture.project.join(".agents/skills.lock.yaml")).unwrap()
}

/// Wraps the system `git` binary behind a directory that can be prepended to
/// `PATH`, logging every invocation's arguments to a file so a test can count
/// how many times a subcommand such as `clone` actually ran.
pub(crate) struct GitSpy {
    _dir: TempDir,
    bin_dir: PathBuf,
    log: PathBuf,
}

impl GitSpy {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let log = dir.path().join("git-invocations.log");
        let real_git = which_git();
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> \"{}\"\nexec \"{}\" \"$@\"\n",
            log.display(),
            real_git
        );
        let script_path = bin_dir.join("git");
        fs::write(&script_path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script_path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script_path, permissions).unwrap();
        }
        Self {
            _dir: dir,
            bin_dir,
            log,
        }
    }

    /// A `PATH` value with the spy directory ahead of the real one, so a
    /// command run with it invokes the spy instead of the system `git`.
    pub(crate) fn path_with_spy(&self) -> String {
        let existing = std::env::var("PATH").unwrap_or_default();
        format!("{}:{existing}", self.bin_dir.display())
    }

    /// Number of logged invocations whose arguments include `clone` as a
    /// standalone token, i.e. actual `git clone` calls.
    pub(crate) fn clone_invocations(&self) -> usize {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.split_whitespace().any(|token| token == "clone"))
            .count()
    }

    /// Number of logged invocations that transfer objects from a source, i.e.
    /// `git clone` or `git fetch` calls. Shallow acquisition fetches a
    /// commit-addressed pin into a fresh bare repository instead of cloning,
    /// so a test asserting one acquisition per distinct source must count
    /// both shapes.
    pub(crate) fn acquisition_invocations(&self) -> usize {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter(|line| {
                line.split_whitespace()
                    .any(|token| token == "clone" || token == "fetch")
            })
            .count()
    }
}

fn which_git() -> String {
    let output = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub(crate) fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}
