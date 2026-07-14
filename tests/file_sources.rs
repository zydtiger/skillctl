mod support;

use std::fs;
use support::*;

#[test]
fn file_source_installs_only_skill_md_and_supports_lifecycle() {
    let fixture = Fixture::new();
    write_skill(&fixture.repo, "root-file-skill", "repository-only-data");
    fs::create_dir_all(fixture.repo.join("other")).unwrap();
    fs::write(fixture.repo.join("other/ignored.txt"), "ignored").unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "add root file skill"]);

    assert_ok(fixture.command().arg("init").output().unwrap());
    let output = fixture
        .command()
        .args([
            "add",
            fixture.repo.to_str().unwrap(),
            "--file",
            "SKILL.md",
            "--name",
            "root-file-folder",
            "--json",
        ])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let document = json(&output);
    assert_eq!(document["skills"][0]["source"]["file"], "SKILL.md");
    assert!(document["skills"][0]["source"]["path"].is_null());

    let installed = fixture.project.join(".agents/skills/root-file-folder");
    assert!(installed.join("SKILL.md").is_file());
    assert!(installed.join(".skillctl-managed").is_file());
    assert!(!installed.join("data.txt").exists());
    assert!(!installed.join("other").exists());
    let marker = fs::read_to_string(installed.join(".skillctl-managed")).unwrap();
    assert!(marker.contains("version: 2"));
    assert!(marker.contains("file: SKILL.md"));
    assert_ok(fixture.command().arg("check").output().unwrap());

    fs::write(
        fixture.repo.join("SKILL.md"),
        "---\nname: root-file-skill\ndescription: Updated root file skill.\n---\n\n# Updated\n",
    )
    .unwrap();
    git(&fixture.repo, &["add", "SKILL.md"]);
    git(&fixture.repo, &["commit", "-qm", "update root file skill"]);
    assert_ok(
        fixture
            .command()
            .args(["update", "root-file-folder"])
            .output()
            .unwrap(),
    );
    assert!(fs::read_to_string(installed.join("SKILL.md"))
        .unwrap()
        .contains("# Updated"));

    fs::write(installed.join("SKILL.md"), "locally changed").unwrap();
    let output = fixture
        .command()
        .args(["diff", "root-file-folder", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    assert_eq!(json(&output)["changes"][0]["path"], "SKILL.md");
    assert_eq!(json(&output)["changes"][0]["change"], "modified");
    assert_ok(
        fixture
            .command()
            .args(["sync", "root-file-folder", "--force"])
            .output()
            .unwrap(),
    );
    assert_ok(fixture.command().arg("check").output().unwrap());
}

#[test]
fn file_source_cli_and_schema_rules_are_strict() {
    let fixture = Fixture::new();
    write_skill(&fixture.repo, "root-file-skill", "ignored");
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "add root file skill"]);
    assert_ok(fixture.command().arg("init").output().unwrap());

    let missing_name = fixture
        .command()
        .args(["add", fixture.repo.to_str().unwrap(), "--file", "SKILL.md"])
        .output()
        .unwrap();
    assert_fail(&missing_name);

    let both = fixture
        .command()
        .args([
            "add",
            fixture.repo.to_str().unwrap(),
            "--path",
            ".",
            "--file",
            "SKILL.md",
            "--name",
            "root-file-folder",
        ])
        .output()
        .unwrap();
    assert_fail(&both);

    for file in ["README.md", "../SKILL.md", "/tmp/SKILL.md"] {
        let output = fixture
            .command()
            .args([
                "add",
                fixture.repo.to_str().unwrap(),
                "--file",
                file,
                "--name",
                "root-file-folder",
            ])
            .output()
            .unwrap();
        assert_fail(&output);
    }

    fs::write(
        fixture.project.join(".agents/skills.lock.yaml"),
        "version: 1\nskills:\n  invalid-file:\n    mode: vendored\n    source:\n      repository: repo\n      file: SKILL.md\n      ref: main\n    resolved:\n      commit: 0123456789abcdef0123456789abcdef01234567\n      digest: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n    destination: invalid-file\n",
    )
    .unwrap();
    let output = fixture.command().arg("list").output().unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires lock schema version 2"));
}

#[test]
fn adding_file_source_upgrades_nonempty_version_one_lock() {
    let fixture = Fixture::new();
    fixture.init_add();
    write_skill(&fixture.repo, "root-file-skill", "ignored");
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "add root file skill"]);
    let lock_path = fixture.project.join(".agents/skills.lock.yaml");
    let version_one =
        fs::read_to_string(&lock_path)
            .unwrap()
            .replacen("version: 2", "version: 1", 1);
    fs::write(&lock_path, version_one).unwrap();
    assert_ok(
        fixture
            .command()
            .args([
                "add",
                fixture.repo.to_str().unwrap(),
                "--file",
                "SKILL.md",
                "--name",
                "root-file-folder",
            ])
            .output()
            .unwrap(),
    );
    assert!(fs::read_to_string(&lock_path)
        .unwrap()
        .starts_with("version: 2\n"));
    assert_ok(fixture.command().arg("check").output().unwrap());
}
