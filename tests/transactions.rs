mod support;

use std::fs;
use support::*;

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

/// A pin-only entry must not be left with its lock ahead of its marker when a
/// later content update fails after an earlier one already rewrote the lock.
#[test]
#[cfg(unix)]
fn pin_only_entries_stay_consistent_when_a_later_update_fails() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    // `demo` stays unchanged (pin-only). `mid` installs successfully and
    // rewrites the whole lock. `zzz` then fails, aborting the run.
    for name in ["mid", "zzz"] {
        write_skill(&fixture.repo.join(format!("skills/{name}")), name, "one");
    }
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "add more skills"]);
    fixture.init_add();
    for name in ["mid", "zzz"] {
        assert_ok(
            fixture
                .command()
                .args([
                    "add",
                    fixture.repo.to_str().unwrap(),
                    "--path",
                    &format!("skills/{name}"),
                ])
                .output()
                .unwrap(),
        );
    }

    for name in ["mid", "zzz"] {
        write_skill(&fixture.repo.join(format!("skills/{name}")), name, "two");
    }
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "change mid and zzz"]);

    // Deny writes inside `zzz`'s destination so its install fails while
    // cleaning up, after `mid` has already installed and rewritten the whole
    // lock. The failure must land after a successful install, not before one.
    let zzz = fixture.project.join(".agents/skills/zzz");
    let mut permissions = fs::metadata(&zzz).unwrap().permissions();
    permissions.set_mode(0o500);
    fs::set_permissions(&zzz, permissions).unwrap();

    let output = fixture.command().arg("update").output().unwrap();

    let mut permissions = fs::metadata(&zzz).unwrap().permissions();
    permissions.set_mode(0o755);
    let _ = fs::set_permissions(&zzz, permissions);

    assert_fail(&output);
    // `demo` is pin-only and was never touched, so its lock entry and its
    // marker must still agree even though the run aborted.
    assert_ok(fixture.command().args(["check", "demo"]).output().unwrap());
}
