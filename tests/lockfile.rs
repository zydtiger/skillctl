mod support;

use std::fs;
use support::*;

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

    fs::write(agents.join("skills.lock.yaml"), "version: 3\nskills: {}\n").unwrap();
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
fn version_one_directory_locks_remain_supported() {
    let fixture = Fixture::new();
    fixture.init_add();
    let lock_path = fixture.project.join(".agents/skills.lock.yaml");
    let lock = fs::read_to_string(&lock_path)
        .unwrap()
        .replacen("version: 2", "version: 1", 1);
    fs::write(&lock_path, lock).unwrap();
    assert_ok(fixture.command().arg("check").output().unwrap());
    assert_ok(fixture.command().arg("list").output().unwrap());
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
