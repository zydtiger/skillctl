mod support;

use std::fs;
use support::*;

#[test]
fn init_is_explicit_and_parent_discovery_works() {
    let fixture = Fixture::new();
    let nested = fixture.project.join("nested/deeper");
    fs::create_dir_all(&nested).unwrap();
    assert_ok(fixture.command().arg("init").output().unwrap());
    assert!(
        fs::read_to_string(fixture.project.join(".agents/skills.lock.yaml"))
            .unwrap()
            .starts_with("version: 2\n")
    );
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
