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
fn generated_bytecode_caches_never_make_a_skill_look_modified() {
    let fixture = Fixture::new();
    fixture.init_add();
    let installed = fixture.project.join(".agents/skills/demo");
    // The cache goes beside files that already exist, as an interpreter would
    // write it. Only the cache directory is ignored, so inventing a parent for
    // it here would add that parent to the tree and change the digest.
    let cache = installed.join("__pycache__");
    fs::create_dir(&cache).unwrap();
    fs::write(cache.join("helper.cpython-312.pyc"), b"compiled").unwrap();

    let output = fixture
        .command()
        .args(["check", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    assert_eq!(json(&output)["skills"][0]["state"], "clean");
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
fn update_advances_pin_without_reinstalling_unchanged_content() {
    let fixture = Fixture::new();
    fixture.init_add();
    let installed = fixture.project.join(".agents/skills/demo/data.txt");
    let before = file_identity(&installed);
    let commit = fixture.advance_unrelated("unrelated change");

    let output = fixture
        .command()
        .args(["update", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let change = json(&output)["changes"][0].clone();
    assert_eq!(change["action"], "pin-only");
    assert_eq!(change["name"], "demo");
    assert_eq!(change["commit"], commit);
    assert_eq!(change["dry_run"], false);

    // The destination must not be replaced: content and inode both survive.
    assert_eq!(file_identity(&installed), before);
    // The advanced commit is recorded so the entry returns to `current`.
    assert!(read_lock(&fixture).contains(&commit));

    let output = fixture
        .command()
        .args(["check", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let skill = &json(&output)["skills"][0];
    assert_eq!(skill["upstream_status"], "current");
    assert_eq!(skill["local_status"], "clean");
}

#[test]
fn update_separates_pin_only_entries_from_changed_content() {
    let fixture = Fixture::new();
    write_skill(&fixture.repo.join("skills/other"), "other", "one");
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "add other skill"]);
    fixture.init_add();
    assert_ok(
        fixture
            .command()
            .args([
                "add",
                fixture.repo.to_str().unwrap(),
                "--path",
                "skills/other",
            ])
            .output()
            .unwrap(),
    );
    let untouched = fixture.project.join(".agents/skills/demo/data.txt");
    let before = file_identity(&untouched);

    write_skill(&fixture.repo.join("skills/other"), "other", "two");
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "change other only"]);
    let commit = git_stdout(&fixture.repo, &["rev-parse", "HEAD"]);

    let output = fixture
        .command()
        .args(["update", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let changes = json(&output)["changes"].as_array().unwrap().clone();
    let actions: Vec<(String, String)> = changes
        .iter()
        .map(|change| {
            (
                change["name"].as_str().unwrap().to_owned(),
                change["action"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(actions.contains(&("demo".to_owned(), "pin-only".to_owned())));
    assert!(actions.contains(&("other".to_owned(), "update".to_owned())));

    // The untouched skill keeps its installed file; the changed one updates.
    assert_eq!(file_identity(&untouched), before);
    assert_eq!(
        fs::read_to_string(fixture.project.join(".agents/skills/other/data.txt")).unwrap(),
        "two"
    );
    // Both entries end up pinned at the advanced commit.
    assert_eq!(read_lock(&fixture).matches(&commit).count(), 2);
}

#[test]
fn update_reinstalls_a_missing_destination_instead_of_advancing_the_pin_only() {
    let fixture = Fixture::new();
    fixture.init_add();
    let installed = fixture.project.join(".agents/skills/demo");
    fs::remove_dir_all(&installed).unwrap();
    fixture.advance_unrelated("unrelated change");

    let output = fixture
        .command()
        .args(["update", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    // A missing destination must be restored rather than merely repinned.
    assert_eq!(json(&output)["changes"][0]["action"], "update");
    assert_eq!(
        fs::read_to_string(installed.join("data.txt")).unwrap(),
        "one"
    );
    assert_ok(fixture.command().arg("check").output().unwrap());
}

#[test]
fn pin_only_update_does_not_require_force_for_modified_content() {
    let fixture = Fixture::new();
    fixture.init_add();
    let installed = fixture.project.join(".agents/skills/demo/data.txt");
    fs::write(&installed, "locally modified").unwrap();
    let commit = fixture.advance_unrelated("unrelated change");

    let output = fixture
        .command()
        .args(["update", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    assert_eq!(json(&output)["changes"][0]["action"], "pin-only");
    // Local modifications are preserved because nothing is replaced.
    assert_eq!(fs::read_to_string(&installed).unwrap(), "locally modified");
    assert!(read_lock(&fixture).contains(&commit));
}

#[test]
fn pin_only_update_dry_run_leaves_scope_unchanged() {
    let fixture = Fixture::new();
    fixture.init_add();
    let lock_path = fixture.project.join(".agents/skills.lock.yaml");
    let old_lock = fs::read(&lock_path).unwrap();
    let installed = fixture.project.join(".agents/skills/demo/data.txt");
    let before = file_identity(&installed);
    let commit = fixture.advance_unrelated("unrelated change");

    let output = fixture
        .command()
        .args(["update", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let change = json(&output)["changes"][0].clone();
    assert_eq!(change["action"], "pin-only");
    assert_eq!(change["commit"], commit);
    assert_eq!(change["dry_run"], true);
    assert_eq!(fs::read(&lock_path).unwrap(), old_lock);
    assert_eq!(file_identity(&installed), before);
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
