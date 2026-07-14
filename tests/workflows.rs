mod support;

use std::fs;
use support::*;

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
