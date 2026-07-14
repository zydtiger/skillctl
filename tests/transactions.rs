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
