use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    repo: PathBuf,
    project: PathBuf,
}

impl Fixture {
    fn new() -> Self {
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

    fn command(&self) -> Command {
        command_in(&self.project)
    }

    fn init_add(&self) {
        assert_ok(self.command().arg("init").output().unwrap());
        self.init_add_after_existing_init();
    }

    fn init_add_after_existing_init(&self) {
        assert_ok(
            self.command()
                .args(["add", self.repo.to_str().unwrap(), "--path", "skills/demo"])
                .output()
                .unwrap(),
        );
    }

    fn advance(&self, content: &str) -> String {
        fs::write(self.repo.join("skills/demo/data.txt"), content).unwrap();
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "-qm", "advance"]);
        git_stdout(&self.repo, &["rev-parse", "HEAD"])
    }
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_skillctl")
}

fn command_in(directory: &Path) -> Command {
    let mut command = Command::new(binary());
    command.current_dir(directory);
    command
}

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert_ok(output);
}

fn git_stdout(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert_ok_ref(&output);
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn assert_ok(output: Output) {
    assert_ok_ref(&output);
}

fn assert_ok_ref(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_fail(output: &Output) {
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn write_skill(directory: &Path, name: &str, data: &str) {
    fs::create_dir_all(directory).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test skill {name}.\n---\n\n# Test\n"),
    )
    .unwrap();
    fs::write(directory.join("data.txt"), data).unwrap();
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn init_is_explicit_and_parent_discovery_works() {
    let fixture = Fixture::new();
    let nested = fixture.project.join("nested/deeper");
    fs::create_dir_all(&nested).unwrap();
    assert_ok(fixture.command().arg("init").output().unwrap());
    let output = command_in(&nested)
        .args(["list", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    assert_eq!(json(&output)["scope"], "project");

    let child = fixture.project.join("child");
    fs::create_dir_all(&child).unwrap();
    assert_ok(command_in(&child).arg("init").output().unwrap());
    assert!(child.join(".agents/skills.lock.yaml").is_file());
    assert_fail(&command_in(&child).arg("init").output().unwrap());
}

#[test]
fn global_scope_uses_isolated_home() {
    let fixture = Fixture::new();
    let home = fixture._temp.path().join("home");
    fs::create_dir(&home).unwrap();
    let output = fixture
        .command()
        .env("SKILLCTL_HOME", &home)
        .args(["init", "--global", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    assert_eq!(json(&output)["scope"], "global");
    assert!(home.join(".agents/skills.lock.yaml").is_file());
    assert!(!fixture.project.join(".agents").exists());

    let output = fixture
        .command()
        .env("SKILLCTL_HOME", &home)
        .args([
            "add",
            fixture.repo.to_str().unwrap(),
            "--path",
            "skills/demo",
            "--global",
            "--json",
        ])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    assert!(home.join(".agents/skills/demo/SKILL.md").is_file());
    assert_ok(
        fixture
            .command()
            .env("SKILLCTL_HOME", &home)
            .args(["check", "--global"])
            .output()
            .unwrap(),
    );
}

#[test]
fn strict_lock_schema_and_unsafe_paths_are_rejected() {
    let fixture = Fixture::new();
    let agents = fixture.project.join(".agents");
    fs::create_dir_all(&agents).unwrap();
    fs::write(
        agents.join("skills.lock.yaml"),
        "version: 1\nunknown: true\nskills: {}\n",
    )
    .unwrap();
    let output = fixture.command().arg("list").output().unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown field"));

    fs::write(agents.join("skills.lock.yaml"), "version: 2\nskills: {}\n").unwrap();
    let output = fixture.command().arg("list").output().unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported"));

    for destination in ["/tmp/escape", "../escape", "a/../escape", "a//b"] {
        fs::write(
            agents.join("skills.lock.yaml"),
            format!(
                "version: 1\nskills:\n  local:\n    mode: local\n    destination: '{destination}'\n"
            ),
        )
        .unwrap();
        assert_fail(&fixture.command().arg("list").output().unwrap());
    }
}

#[test]
fn add_check_list_status_and_json_are_consistent() {
    let fixture = Fixture::new();
    fixture.init_add();
    assert!(fixture
        .project
        .join(".agents/skills/demo/.skillctl-managed")
        .is_file());
    for command_name in ["check", "status", "list"] {
        let output = fixture
            .command()
            .args([command_name, "--json"])
            .output()
            .unwrap();
        assert_ok_ref(&output);
        let document = json(&output);
        assert_eq!(document["ok"], true);
        assert_eq!(document["skills"][0]["name"], "demo");
    }
}

#[test]
fn root_source_path_and_distinct_destination_name_are_supported() {
    let fixture = Fixture::new();
    write_skill(&fixture.repo, "root-skill", "root");
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "root skill"]);
    assert_ok(fixture.command().arg("init").output().unwrap());
    let output = fixture
        .command()
        .args([
            "add",
            fixture.repo.to_str().unwrap(),
            "--path",
            ".",
            "--name",
            "root-skill-folder",
            "--json",
        ])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    assert_eq!(json(&output)["skills"][0]["name"], "root-skill-folder");
    assert_ok(fixture.command().arg("check").output().unwrap());
}

#[test]
fn check_is_offline_and_detects_missing_modified_and_marker_changes() {
    let fixture = Fixture::new();
    fixture.init_add();
    let installed = fixture.project.join(".agents/skills/demo");
    fs::write(installed.join("data.txt"), "changed").unwrap();
    let output = fixture
        .command()
        .args(["check", "--json"])
        .output()
        .unwrap();
    assert_fail(&output);
    let document = json(&output);
    assert_eq!(document["ok"], false);
    assert_eq!(document["skills"][0]["state"], "modified");

    assert_ok(
        fixture
            .command()
            .args(["sync", "--force"])
            .output()
            .unwrap(),
    );
    fs::remove_dir_all(&fixture.repo).unwrap();
    assert_ok(fixture.command().arg("check").output().unwrap());

    fs::write(installed.join(".skillctl-managed"), "version: 1\n").unwrap();
    let output = fixture.command().arg("check").output().unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("marker"));

    let output = fixture.command().arg("status").output().unwrap();
    assert_ok_ref(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("invalid"));

    fs::remove_dir_all(&installed).unwrap();
    let output = fixture.command().arg("check").output().unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing"));
}

#[test]
fn sync_restores_exact_commit_and_requires_force_for_changes() {
    let fixture = Fixture::new();
    fixture.init_add();
    let installed = fixture.project.join(".agents/skills/demo");
    fs::write(installed.join("data.txt"), "locally changed").unwrap();
    let refused = fixture.command().arg("sync").output().unwrap();
    assert_fail(&refused);
    assert_eq!(
        fs::read_to_string(installed.join("data.txt")).unwrap(),
        "locally changed"
    );

    let dry = fixture
        .command()
        .args(["sync", "--force", "--dry-run"])
        .output()
        .unwrap();
    assert_ok_ref(&dry);
    assert_eq!(
        fs::read_to_string(installed.join("data.txt")).unwrap(),
        "locally changed"
    );

    assert_ok(
        fixture
            .command()
            .args(["sync", "--force"])
            .output()
            .unwrap(),
    );
    assert_eq!(
        fs::read_to_string(installed.join("data.txt")).unwrap(),
        "one"
    );

    fixture.advance("new branch content");
    fs::remove_dir_all(&installed).unwrap();
    assert_ok(fixture.command().arg("sync").output().unwrap());
    assert_eq!(
        fs::read_to_string(installed.join("data.txt")).unwrap(),
        "one"
    );
}

#[test]
fn add_and_update_dry_runs_leave_scope_unchanged() {
    let fixture = Fixture::new();
    assert_ok(fixture.command().arg("init").output().unwrap());
    let lock_path = fixture.project.join(".agents/skills.lock.yaml");
    let empty_lock = fs::read(&lock_path).unwrap();
    let output = fixture
        .command()
        .args([
            "add",
            fixture.repo.to_str().unwrap(),
            "--path",
            "skills/demo",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    assert_eq!(fs::read(&lock_path).unwrap(), empty_lock);
    assert!(!fixture.project.join(".agents/skills/demo").exists());

    fixture.init_add_after_existing_init();
    let old_lock = fs::read(&lock_path).unwrap();
    fixture.advance("two");
    assert_ok(
        fixture
            .command()
            .args(["update", "--dry-run"])
            .output()
            .unwrap(),
    );
    assert_eq!(fs::read(&lock_path).unwrap(), old_lock);
    assert_eq!(
        fs::read_to_string(fixture.project.join(".agents/skills/demo/data.txt")).unwrap(),
        "one"
    );
}

#[test]
fn update_advances_branch_and_then_is_a_no_op() {
    let fixture = Fixture::new();
    fixture.init_add();
    let commit = fixture.advance("two");
    let output = fixture
        .command()
        .args(["update", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    assert_eq!(json(&output)["changes"][0]["commit"], commit);
    assert_eq!(
        fs::read_to_string(fixture.project.join(".agents/skills/demo/data.txt")).unwrap(),
        "two"
    );
    let output = fixture
        .command()
        .args(["update", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    assert_eq!(json(&output)["changes"][0]["action"], "no-op");
}

#[test]
fn diff_reports_deterministic_path_records() {
    let fixture = Fixture::new();
    fixture.init_add();
    let installed = fixture.project.join(".agents/skills/demo");
    fs::write(installed.join("data.txt"), "changed").unwrap();
    fs::write(installed.join("extra.txt"), "extra").unwrap();
    let output = fixture.command().args(["diff", "--json"]).output().unwrap();
    assert_ok_ref(&output);
    let changes = json(&output)["changes"].as_array().unwrap().clone();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0]["path"], "data.txt");
    assert_eq!(changes[0]["change"], "modified");
    assert_eq!(changes[1]["path"], "extra.txt");
    assert_eq!(changes[1]["change"], "added");
}

#[test]
fn local_entries_are_never_mutated_and_remove_keeps_files() {
    let fixture = Fixture::new();
    assert_ok(fixture.command().arg("init").output().unwrap());
    let local = fixture.project.join(".agents/skills/local-one");
    write_skill(&local, "local-one", "owned");
    fs::write(
        fixture.project.join(".agents/skills.lock.yaml"),
        "version: 1\nskills:\n  local-one:\n    mode: local\n    destination: local-one\n",
    )
    .unwrap();
    assert_ok(fixture.command().arg("check").output().unwrap());
    assert_ok(fixture.command().arg("sync").output().unwrap());
    assert_eq!(fs::read_to_string(local.join("data.txt")).unwrap(), "owned");
    let dry = fixture
        .command()
        .args(["remove", "local-one", "--dry-run"])
        .output()
        .unwrap();
    assert_ok_ref(&dry);
    assert!(local.exists());
    assert_ok(
        fixture
            .command()
            .args(["remove", "local-one"])
            .output()
            .unwrap(),
    );
    assert!(local.exists());
    assert!(
        !fs::read_to_string(fixture.project.join(".agents/skills.lock.yaml"))
            .unwrap()
            .contains("local-one")
    );
}

#[test]
fn remove_refuses_unmanaged_vendored_content() {
    let fixture = Fixture::new();
    fixture.init_add();
    fs::remove_file(
        fixture
            .project
            .join(".agents/skills/demo/.skillctl-managed"),
    )
    .unwrap();
    let output = fixture.command().args(["remove", "demo"]).output().unwrap();
    assert_fail(&output);
    assert!(fixture.project.join(".agents/skills/demo").exists());
}

#[test]
fn transaction_rolls_back_destination_when_lock_write_fails() {
    let fixture = Fixture::new();
    fixture.init_add();
    let lock_path = fixture.project.join(".agents/skills.lock.yaml");
    let old_lock = fs::read(&lock_path).unwrap();
    fixture.advance("two");
    let output = fixture
        .command()
        .env("SKILLCTL_TEST_FAIL_LOCK_WRITE", "1")
        .arg("update")
        .output()
        .unwrap();
    assert_fail(&output);
    assert_eq!(fs::read(&lock_path).unwrap(), old_lock);
    assert_eq!(
        fs::read_to_string(fixture.project.join(".agents/skills/demo/data.txt")).unwrap(),
        "one"
    );
}

#[cfg(unix)]
#[test]
fn source_symlinks_and_installed_special_files_are_rejected() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    symlink("data.txt", fixture.repo.join("skills/demo/link")).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "symlink"]);
    assert_ok(fixture.command().arg("init").output().unwrap());
    let output = fixture
        .command()
        .args([
            "add",
            fixture.repo.to_str().unwrap(),
            "--path",
            "skills/demo",
        ])
        .output()
        .unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported"));
}

#[cfg(unix)]
#[test]
fn scope_symlinks_and_special_files_are_rejected() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = fixture._temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, fixture.project.join(".agents")).unwrap();
    let output = fixture.command().arg("init").output().unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("symlink"));

    fs::remove_file(fixture.project.join(".agents")).unwrap();
    fixture.init_add();
    let fifo = fixture.project.join(".agents/skills/demo/unsafe-fifo");
    assert_ok(
        Command::new("mkfifo")
            .arg(&fifo)
            .output()
            .expect("mkfifo must be available"),
    );
    let output = fixture.command().arg("check").output().unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("special file"));
}

#[test]
fn duplicate_destinations_and_declared_names_are_reported() {
    let fixture = Fixture::new();
    assert_ok(fixture.command().arg("init").output().unwrap());
    let skills = fixture.project.join(".agents/skills");
    write_skill(&skills.join("one"), "same-name", "one");
    write_skill(&skills.join("two"), "same-name", "two");
    fs::write(
        fixture.project.join(".agents/skills.lock.yaml"),
        "version: 1\nskills:\n  one:\n    mode: local\n    destination: one\n  two:\n    mode: local\n    destination: two\n",
    )
    .unwrap();
    let output = fixture.command().arg("check").output().unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("duplicated"));

    fs::write(
        fixture.project.join(".agents/skills.lock.yaml"),
        "version: 1\nskills:\n  one:\n    mode: local\n    destination: same\n  two:\n    mode: local\n    destination: same\n",
    )
    .unwrap();
    assert_fail(&fixture.command().arg("list").output().unwrap());

    fs::write(
        fixture.project.join(".agents/skills.lock.yaml"),
        "version: 1\nskills:\n  one:\n    mode: local\n    destination: parent\n  two:\n    mode: local\n    destination: parent/child\n",
    )
    .unwrap();
    let output = fixture.command().arg("list").output().unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("overlapping"));
}

#[test]
fn invalid_global_flags_are_not_silently_ignored() {
    let fixture = Fixture::new();
    assert_ok(fixture.command().arg("init").output().unwrap());
    let output = fixture
        .command()
        .args(["list", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert_fail(&output);
    assert_eq!(json(&output)["ok"], false);
    let output = fixture
        .command()
        .args(["remove", "x", "--force"])
        .output()
        .unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("only valid"));

    let output = fixture.command().args(["add", "--json"]).output().unwrap();
    assert_fail(&output);
    assert_eq!(json(&output)["ok"], false);
}

#[test]
fn command_help_smoke_test_matches_readme_surface() {
    let fixture = Fixture::new();
    let output = fixture.command().arg("--help").output().unwrap();
    assert_ok_ref(&output);
    let help = String::from_utf8(output.stdout).unwrap();
    for command in [
        "init", "add", "sync", "check", "status", "diff", "update", "remove", "list",
    ] {
        assert!(help.contains(command));
    }
}
