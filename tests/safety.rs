mod support;

use std::fs;
use std::process::Command;
use support::*;

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
