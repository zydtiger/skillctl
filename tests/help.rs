mod support;

use support::*;

#[test]
fn command_help_smoke_test_matches_readme_surface() {
    let fixture = Fixture::new();
    let output = fixture.command().arg("--help").output().unwrap();
    assert_ok_ref(&output);
    let help = String::from_utf8(output.stdout).unwrap();
    for command in [
        "init", "add", "sync", "check", "status", "diff", "update", "remove", "list",
    ] {
        assert!(help.contains(command));
    }

    let status = fixture
        .command()
        .args(["status", "--help"])
        .output()
        .unwrap();
    assert_ok_ref(&status);
    assert!(String::from_utf8(status.stdout)
        .unwrap()
        .contains("--offline"));
}
