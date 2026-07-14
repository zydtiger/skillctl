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

pub(crate) fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}
