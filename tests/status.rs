mod support;

use std::fs;
use support::*;

#[test]
fn status_reports_clean_current_with_explicit_and_legacy_json_fields() {
    let fixture = Fixture::new();
    fixture.init_add();

    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let document = json(&output);
    let skill = &document["skills"][0];
    assert_eq!(document["ok"], true);
    assert_eq!(skill["local_status"], "clean");
    assert_eq!(skill["upstream_status"], "current");
    assert_eq!(skill["content_changed"], false);
    assert_eq!(skill["recommended_action"], "none");
    assert_eq!(skill["pinned_commit"], skill["commit"]);
    assert_eq!(skill["upstream_commit"], skill["pinned_commit"]);
    assert_eq!(skill["state"], "clean");
    assert_eq!(skill["update_status"], "current");

    let human = fixture.command().arg("status").output().unwrap();
    assert_ok_ref(&human);
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(stdout.contains("NAME"));
    assert!(stdout.contains("LOCAL"));
    assert!(stdout.contains("UPSTREAM"));
    assert!(stdout.contains("ACTION"));
    assert!(stdout.contains("demo"));
    assert!(stdout.contains("clean"));
    assert!(stdout.contains("current"));
    assert!(!stdout.contains("unknown"));
}

#[test]
fn status_detects_selected_skill_update_by_digest() {
    let fixture = Fixture::new();
    fixture.init_add();
    let upstream_commit = fixture.advance("two");

    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let skill = &json(&output)["skills"][0];
    assert_eq!(skill["local_status"], "clean");
    assert_eq!(skill["upstream_status"], "update_available");
    assert_eq!(skill["upstream_commit"], upstream_commit);
    assert_eq!(skill["content_changed"], true);
    assert_eq!(skill["recommended_action"], "skillctl update demo");
}

#[test]
fn status_distinguishes_source_advance_without_selected_content_change() {
    let fixture = Fixture::new();
    fixture.init_add();
    fs::write(fixture.repo.join("README.md"), "unrelated change").unwrap();
    git(&fixture.repo, &["add", "README.md"]);
    git(&fixture.repo, &["commit", "-qm", "change unrelated file"]);
    let upstream_commit = git_stdout(&fixture.repo, &["rev-parse", "HEAD"]);

    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let skill = &json(&output)["skills"][0];
    assert_eq!(skill["upstream_status"], "source_advanced");
    assert_eq!(skill["upstream_commit"], upstream_commit);
    assert_eq!(skill["content_changed"], false);
    assert_eq!(
        skill["recommended_action"],
        "skillctl update demo (pin only)"
    );
}

#[test]
fn status_evaluates_each_skill_when_shared_repository_ref_advances() {
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

    fs::write(fixture.repo.join("skills/other/data.txt"), "two").unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "change only other skill"]);

    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let document = json(&output);
    let skills = document["skills"].as_array().unwrap();
    let demo = skills.iter().find(|skill| skill["name"] == "demo").unwrap();
    let other = skills
        .iter()
        .find(|skill| skill["name"] == "other")
        .unwrap();
    assert_eq!(demo["upstream_status"], "source_advanced");
    assert_eq!(demo["content_changed"], false);
    assert_eq!(other["upstream_status"], "update_available");
    assert_eq!(other["content_changed"], true);
}

#[test]
fn status_preserves_modified_missing_and_invalid_local_states() {
    let fixture = Fixture::new();
    fixture.init_add();
    let installed = fixture.project.join(".agents/skills/demo");

    fs::write(installed.join("data.txt"), "locally modified").unwrap();
    let modified = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&modified);
    let skill = &json(&modified)["skills"][0];
    assert_eq!(skill["local_status"], "modified");
    assert_eq!(skill["upstream_status"], "current");
    assert_eq!(skill["recommended_action"], "review: skillctl diff demo");

    fs::remove_dir_all(&installed).unwrap();
    let missing = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&missing);
    assert_eq!(json(&missing)["skills"][0]["local_status"], "missing");
    assert_eq!(
        json(&missing)["skills"][0]["recommended_action"],
        "skillctl sync demo"
    );

    assert_ok(fixture.command().arg("sync").output().unwrap());
    fs::write(installed.join(".skillctl-managed"), "not yaml: [").unwrap();
    let invalid = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&invalid);
    assert_eq!(json(&invalid)["skills"][0]["local_status"], "invalid");
    assert!(json(&invalid)["skills"][0]["recommended_action"]
        .as_str()
        .unwrap()
        .contains("--force"));
}

#[test]
fn status_reports_unreachable_without_losing_local_integrity_or_failing() {
    let fixture = Fixture::new();
    fixture.init_add();
    fs::remove_dir_all(&fixture.repo).unwrap();

    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let document = json(&output);
    let skill = &document["skills"][0];
    assert_eq!(document["ok"], true);
    assert_eq!(skill["local_status"], "clean");
    assert_eq!(skill["upstream_status"], "unreachable");
    assert!(skill["upstream_commit"].is_null());
    assert!(skill["content_changed"].is_null());
    assert!(skill["upstream_details"].as_array().unwrap().len() == 1);
    assert_eq!(
        skill["recommended_action"],
        "retry; verify repository/ref access"
    );

    let human = fixture.command().arg("status").output().unwrap();
    assert_ok_ref(&human);
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(stdout.contains("clean"));
    assert!(stdout.contains("unreachable"));
    assert!(stdout.contains("Upstream error:"));
}

#[test]
fn status_offline_skips_upstream_once_and_never_uses_the_source() {
    let fixture = Fixture::new();
    fixture.init_add();
    fs::remove_dir_all(&fixture.repo).unwrap();

    let output = fixture
        .command()
        .args(["status", "--offline", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let skill = &json(&output)["skills"][0];
    assert_eq!(skill["local_status"], "clean");
    assert_eq!(skill["upstream_status"], "not_checked");
    assert!(skill["upstream_details"].as_array().unwrap().is_empty());
    assert!(skill["upstream_commit"].is_null());

    let human = fixture
        .command()
        .args(["status", "--offline"])
        .output()
        .unwrap();
    assert_ok_ref(&human);
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert_eq!(stdout.matches("Upstream checks were skipped").count(), 1);
    assert!(!stdout.contains("Upstream error:"));
}

#[test]
fn check_is_a_concise_network_free_integrity_gate_with_nonzero_failures() {
    let fixture = Fixture::new();
    fixture.init_add();
    fs::remove_dir_all(&fixture.repo).unwrap();

    let clean = fixture.command().arg("check").output().unwrap();
    assert_ok_ref(&clean);
    let stdout = String::from_utf8(clean.stdout).unwrap();
    assert!(stdout.contains("OK: 1 lock entry passed offline integrity checks"));
    assert!(!stdout.contains("upstream"));
    assert!(!stdout.contains("unknown"));

    fs::write(
        fixture.project.join(".agents/skills/demo/data.txt"),
        "modified",
    )
    .unwrap();
    let failed = fixture
        .command()
        .args(["check", "--json"])
        .output()
        .unwrap();
    assert_fail(&failed);
    let document = json(&failed);
    assert_eq!(document["ok"], false);
    assert_eq!(document["skills"][0]["state"], "modified");
    assert!(document["errors"][0].as_str().unwrap().contains("digest"));
}

#[test]
fn local_entries_are_project_owned_and_have_no_upstream_dimension() {
    let fixture = Fixture::new();
    assert_ok(fixture.command().arg("init").output().unwrap());
    let local = fixture.project.join(".agents/skills/local-one");
    write_skill(&local, "local-one", "owned");
    fs::write(
        fixture.project.join(".agents/skills.lock.yaml"),
        "version: 2\nskills:\n  local-one:\n    mode: local\n    destination: local-one\n",
    )
    .unwrap();

    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let skill = &json(&output)["skills"][0];
    assert_eq!(skill["local_status"], "local");
    assert_eq!(skill["upstream_status"], "not_applicable");
    assert_eq!(skill["recommended_action"], "project-owned; manage locally");
}
