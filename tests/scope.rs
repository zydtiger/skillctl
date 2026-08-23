mod support;

use std::fs;
use std::path::Path;
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

/// The global installation is structurally identical to a project lock rooted at
/// the home directory, so the upward walk must skip that one root instead of
/// adopting it and misreporting every managed marker.
#[test]
fn bare_command_under_home_reports_scope_error_with_global_redirect() {
    let fixture = Fixture::new();
    let home = fixture._temp.path().join("home");
    fs::create_dir(&home).unwrap();
    install_global_skill(&fixture, &home);

    let work = home.join("work/nested");
    fs::create_dir_all(&work).unwrap();
    let output = command_in(&work)
        .env("SKILLCTL_HOME", &home)
        .arg("check")
        .output()
        .unwrap();
    assert_fail(&output);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("no project .agents/skills.lock.yaml found"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "the global installation exists at {}",
            home.join(".agents/skills.lock.yaml").display()
        )),
        "{stderr}"
    );
    assert!(stderr.contains("use --global to operate on it"), "{stderr}");
    // The previous failure mode reported every global entry as a marker mismatch.
    assert!(!stderr.contains("managed marker"), "{stderr}");
    assert!(!stderr.contains("integrity check failed"), "{stderr}");

    // Scope stays an explicit input: the same directory works with --global.
    assert_ok(
        command_in(&work)
            .env("SKILLCTL_HOME", &home)
            .args(["check", "--global"])
            .output()
            .unwrap(),
    );
}

#[test]
fn bare_command_without_global_installation_keeps_init_guidance_alone() {
    let fixture = Fixture::new();
    let home = fixture._temp.path().join("home");
    let work = home.join("work");
    fs::create_dir_all(&work).unwrap();

    let output = command_in(&work)
        .env("SKILLCTL_HOME", &home)
        .arg("check")
        .output()
        .unwrap();
    assert_fail(&output);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("no project .agents/skills.lock.yaml found"),
        "{stderr}"
    );
    assert!(
        stderr.contains("run `skillctl init` in the intended project root"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("the global installation exists at"),
        "{stderr}"
    );
    assert!(!stderr.contains("--global"), "{stderr}");
}

#[test]
fn bare_commands_still_resolve_a_project_under_the_home_directory() {
    let fixture = Fixture::new();
    let home = fixture._temp.path().join("home");
    fs::create_dir(&home).unwrap();
    install_global_skill(&fixture, &home);

    let project = home.join("projects/app");
    let nested = project.join("nested");
    fs::create_dir_all(&nested).unwrap();
    assert_ok(
        command_in(&project)
            .env("SKILLCTL_HOME", &home)
            .arg("init")
            .output()
            .unwrap(),
    );
    assert_ok(
        command_in(&project)
            .env("SKILLCTL_HOME", &home)
            .args([
                "add",
                fixture.repo.to_str().unwrap(),
                "--path",
                "skills/demo",
            ])
            .output()
            .unwrap(),
    );

    let output = command_in(&nested)
        .env("SKILLCTL_HOME", &home)
        .args(["list", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let document = json(&output);
    assert_eq!(document["scope"], "project");
    assert!(
        document["lock_file"]
            .as_str()
            .unwrap()
            .ends_with("projects/app/.agents/skills.lock.yaml"),
        "{}",
        document["lock_file"]
    );
    assert_ok(
        command_in(&nested)
            .env("SKILLCTL_HOME", &home)
            .arg("check")
            .output()
            .unwrap(),
    );
}

/// A globally scoped snapshot hand-copied into a real project is a scope
/// disagreement, not generic marker/lock corruption.
#[test]
fn marker_scope_disagreement_is_reported_as_its_own_diagnostic() {
    let fixture = Fixture::new();
    let home = fixture._temp.path().join("home");
    fs::create_dir(&home).unwrap();
    fixture.init_add();

    let marker_file = fixture
        .project
        .join(".agents/skills/demo/.skillctl-managed");
    let marker = fs::read_to_string(&marker_file).unwrap();
    assert!(marker.contains("scope: project"), "{marker}");
    fs::write(
        &marker_file,
        marker.replace("scope: project", "scope: global"),
    )
    .unwrap();

    let output = fixture
        .command()
        .env("SKILLCTL_HOME", &home)
        .arg("check")
        .output()
        .unwrap();
    assert_fail(&output);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains(
            "managed marker declares global scope but this lock is checked as project scope"
        ),
        "{stderr}"
    );
    assert!(
        !stderr.contains("managed marker does not match the lock entry"),
        "{stderr}"
    );
}

fn install_global_skill(fixture: &Fixture, home: &Path) {
    assert_ok(
        fixture
            .command()
            .env("SKILLCTL_HOME", home)
            .args(["init", "--global"])
            .output()
            .unwrap(),
    );
    assert_ok(
        fixture
            .command()
            .env("SKILLCTL_HOME", home)
            .args([
                "add",
                fixture.repo.to_str().unwrap(),
                "--path",
                "skills/demo",
                "--global",
            ])
            .output()
            .unwrap(),
    );
}
