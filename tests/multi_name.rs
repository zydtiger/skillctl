mod support;

use std::fs;
use std::path::PathBuf;
use support::*;

/// Add a second vendored entry, `extra`, from the same repository/ref as the
/// `demo` entry `fixture.init_add()` installs. An unrelated advance of that
/// shared repository therefore leaves `extra`'s own content untouched while
/// still moving the ref it tracks, the shape that produces a pin-only
/// (`source_advanced`) upstream state.
fn add_extra(fixture: &Fixture) {
    write_skill(&fixture.repo.join("skills/extra"), "extra", "one");
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "add extra skill"]);
    assert_ok(
        fixture
            .command()
            .args([
                "add",
                fixture.repo.to_str().unwrap(),
                "--path",
                "skills/extra",
            ])
            .output()
            .unwrap(),
    );
}

/// Add a third vendored entry, `other`, from a genuinely distinct second
/// repository, so it is never entangled with a `demo`/`extra` ref advance.
/// Returns that repository's path so a test can advance it independently.
fn add_other(fixture: &Fixture) -> PathBuf {
    let repo2 = fixture._temp.path().join("source2");
    fs::create_dir_all(repo2.join("skills/other")).unwrap();
    git(&repo2, &["init", "-q", "-b", "main"]);
    git(&repo2, &["config", "user.email", "tests@example.com"]);
    git(&repo2, &["config", "user.name", "Tests"]);
    write_skill(&repo2.join("skills/other"), "other", "one");
    git(&repo2, &["add", "."]);
    git(&repo2, &["commit", "-qm", "initial"]);
    assert_ok(
        fixture
            .command()
            .args(["add", repo2.to_str().unwrap(), "--path", "skills/other"])
            .output()
            .unwrap(),
    );
    repo2
}

/// Add both `extra` and `other` on top of `demo`, giving a test three
/// entries: two (`demo`, `extra`) sharing a repository/ref and one (`other`)
/// from a genuinely distinct source. Mirrors the multi-entry setup used
/// throughout `tests/workflows.rs`.
fn add_extra_and_other(fixture: &Fixture) -> PathBuf {
    add_extra(fixture);
    add_other(fixture)
}

#[test]
fn multi_name_update_selects_exactly_the_named_subset() {
    let fixture = Fixture::new();
    fixture.init_add();
    let repo2 = add_extra_and_other(&fixture);

    // Advance every source so all three entries have upstream content
    // changes available, then update only two of the three by name.
    write_skill(&fixture.repo.join("skills/demo"), "demo", "two");
    write_skill(&fixture.repo.join("skills/extra"), "extra", "two");
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "advance demo and extra"]);
    write_skill(&repo2.join("skills/other"), "other", "two");
    git(&repo2, &["add", "."]);
    git(&repo2, &["commit", "-qm", "advance other"]);

    let output = fixture
        .command()
        .args(["update", "demo", "extra", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let changes = json(&output)["changes"].as_array().unwrap().clone();
    let names: Vec<String> = changes
        .iter()
        .map(|change| change["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, vec!["demo".to_owned(), "extra".to_owned()]);

    assert_eq!(
        fs::read_to_string(fixture.project.join(".agents/skills/demo/data.txt")).unwrap(),
        "two"
    );
    assert_eq!(
        fs::read_to_string(fixture.project.join(".agents/skills/extra/data.txt")).unwrap(),
        "two"
    );
    // `other` was never named, so it must stay at its original content and
    // pin even though its own upstream had a change available too.
    assert_eq!(
        fs::read_to_string(fixture.project.join(".agents/skills/other/data.txt")).unwrap(),
        "one"
    );
}

#[test]
fn unknown_name_among_valid_names_fails_before_any_mutation() {
    let fixture = Fixture::new();
    fixture.init_add();
    let lock_path = fixture.project.join(".agents/skills.lock.yaml");
    let old_lock = fs::read(&lock_path).unwrap();
    let installed = fixture.project.join(".agents/skills/demo/data.txt");
    let before = file_identity(&installed);
    // An update would actually change something if it ran, so the assertions
    // below prove the failure aborted before any write rather than merely
    // having nothing to do.
    fixture.advance("two");

    let output = fixture
        .command()
        .args(["update", "demo", "does-not-exist"])
        .output()
        .unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("lock entry `does-not-exist` does not exist"));
    assert_eq!(fs::read(&lock_path).unwrap(), old_lock);
    assert_eq!(file_identity(&installed), before);

    // The same rule applies to sync, check, and diff.
    let sync_output = fixture
        .command()
        .args(["sync", "demo", "does-not-exist"])
        .output()
        .unwrap();
    assert_fail(&sync_output);
    assert_eq!(fs::read(&lock_path).unwrap(), old_lock);

    let check_output = fixture
        .command()
        .args(["check", "demo", "does-not-exist"])
        .output()
        .unwrap();
    assert_fail(&check_output);

    let diff_output = fixture
        .command()
        .args(["diff", "demo", "does-not-exist"])
        .output()
        .unwrap();
    assert_fail(&diff_output);
}

#[test]
fn duplicate_names_are_processed_once() {
    let fixture = Fixture::new();
    fixture.init_add();
    let commit = fixture.advance("two");

    let output = fixture
        .command()
        .args(["update", "demo", "demo", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let changes = json(&output)["changes"].as_array().unwrap().clone();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["name"], "demo");
    assert_eq!(changes[0]["commit"], commit);
}

#[test]
fn explicitly_naming_a_local_entry_fails_and_leaves_named_vendored_entries_untouched() {
    let fixture = Fixture::new();
    fixture.init_add();
    let local = fixture.project.join(".agents/skills/local-one");
    write_skill(&local, "local-one", "owned");
    let lock_path = fixture.project.join(".agents/skills.lock.yaml");
    let mut lock = fs::read_to_string(&lock_path).unwrap();
    lock.push_str("  local-one:\n    mode: local\n    destination: local-one\n");
    fs::write(&lock_path, &lock).unwrap();
    assert_ok(fixture.command().arg("check").output().unwrap());

    let installed = fixture.project.join(".agents/skills/demo/data.txt");
    let before = file_identity(&installed);
    fixture.advance("two");

    let output = fixture
        .command()
        .args(["update", "local-one", "demo"])
        .output()
        .unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("cannot update local entry `local-one`; local files are project-owned"));
    // The vendored entry named alongside it must stay untouched too: every
    // name is validated and acquisition/mutation only follow afterward.
    assert_eq!(file_identity(&installed), before);

    // The bare, all-entries form still sweeps the local entry silently.
    let bare = fixture.command().arg("update").output().unwrap();
    assert_ok_ref(&bare);
    assert!(
        String::from_utf8_lossy(&bare.stdout).contains("left local entry `local-one` untouched")
    );
}

/// Add a `mode: local` entry, `local-one`, next to the vendored entries
/// `fixture.init_add()` (and optionally `add_extra`/`add_other`) already
/// installed, by appending its declaration to the generated lock file.
fn add_local_entry(fixture: &Fixture) {
    let local = fixture.project.join(".agents/skills/local-one");
    write_skill(&local, "local-one", "owned");
    let lock_path = fixture.project.join(".agents/skills.lock.yaml");
    let mut lock = fs::read_to_string(&lock_path).unwrap();
    lock.push_str("  local-one:\n    mode: local\n    destination: local-one\n");
    fs::write(&lock_path, &lock).unwrap();
    assert_ok(fixture.command().arg("check").output().unwrap());
}

#[test]
fn explicitly_naming_a_local_entry_fails_for_sync() {
    let fixture = Fixture::new();
    fixture.init_add();
    add_local_entry(&fixture);

    let installed = fixture.project.join(".agents/skills/demo/data.txt");
    fs::write(&installed, "locally modified").unwrap();

    let output = fixture
        .command()
        .args(["sync", "local-one", "demo"])
        .output()
        .unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("cannot sync local entry `local-one`; local files are project-owned"));
    // `demo` was named alongside it and had a refusable local change, so if
    // sync had reached it at all it would have needed `--force`; it must
    // never be reached, and its content stays exactly as left above.
    assert_eq!(fs::read_to_string(&installed).unwrap(), "locally modified");

    // The bare, all-entries form still leaves the local entry untouched.
    let bare = fixture.command().arg("sync").output().unwrap();
    assert_fail(&bare); // `demo` is still refused without --force.
    assert!(
        String::from_utf8_lossy(&bare.stderr).contains("refusing to replace changed destination")
    );
}

#[test]
fn explicitly_naming_a_local_entry_fails_for_diff() {
    let fixture = Fixture::new();
    fixture.init_add();
    add_local_entry(&fixture);

    let output = fixture
        .command()
        .args(["diff", "local-one", "demo"])
        .output()
        .unwrap();
    assert_fail(&output);
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("cannot diff local entry `local-one` against a pinned source"));

    // The bare, all-entries form still skips the local entry silently and
    // reports on `demo` alone.
    let bare = fixture.command().args(["diff", "--json"]).output().unwrap();
    assert_ok_ref(&bare);
    assert!(json(&bare)["changes"].as_array().unwrap().is_empty());
}

/// A lock with one `mode: local` entry and one `mode: vendored` entry whose
/// source repository does not exist at all. Naming both together must fail
/// with the local-entry error alone: if acquisition of the broken source had
/// even started, the error would instead mention the repository path.
fn write_lock_with_local_entry_and_unreachable_vendored_source(fixture: &Fixture) {
    let local = fixture.project.join(".agents/skills/local-one");
    write_skill(&local, "local-one", "owned");
    fs::write(
        fixture.project.join(".agents/skills.lock.yaml"),
        concat!(
            "version: 2\n",
            "skills:\n",
            "  broken:\n",
            "    mode: vendored\n",
            "    source:\n",
            "      repository: /nonexistent/unreachable-repo\n",
            "      path: skills/broken\n",
            "      ref: main\n",
            "    resolved:\n",
            "      commit: \"111111111111111111111111111111111111111a\"\n",
            "      digest: \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
            "    destination: broken\n",
            "  local-one:\n",
            "    mode: local\n",
            "    destination: local-one\n",
        ),
    )
    .unwrap();
}

#[test]
fn named_local_entry_is_rejected_before_any_acquisition_regardless_of_argument_order() {
    let fixture = Fixture::new();
    assert_ok(fixture.command().arg("init").output().unwrap());
    write_lock_with_local_entry_and_unreachable_vendored_source(&fixture);

    // Name the unreachable vendored entry FIRST and the local entry second:
    // the ordering that previously let `update`/`sync` acquire the broken
    // source before reaching the local-entry bail.
    for command_name in ["update", "sync"] {
        let output = fixture
            .command()
            .args([command_name, "broken", "local-one"])
            .output()
            .unwrap();
        assert_fail(&output);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(&format!(
            "cannot {command_name} local entry `local-one`; local files are project-owned"
        )));
        // No acquisition was attempted: the unreachable repository path
        // never appears anywhere in the failure.
        assert!(!stderr.contains("/nonexistent/unreachable-repo"));
    }

    // `diff` never batches acquisition, but the same argument order must
    // still be rejected before it reaches (and tries to acquire) `broken`.
    let diff_output = fixture
        .command()
        .args(["diff", "broken", "local-one"])
        .output()
        .unwrap();
    assert_fail(&diff_output);
    let diff_stderr = String::from_utf8_lossy(&diff_output.stderr);
    assert!(diff_stderr.contains("cannot diff local entry `local-one` against a pinned source"));
    assert!(!diff_stderr.contains("/nonexistent/unreachable-repo"));
}

#[test]
fn bare_and_single_name_invocations_are_unchanged() {
    let fixture = Fixture::new();
    fixture.init_add();
    add_extra_and_other(&fixture);
    write_skill(&fixture.repo.join("skills/demo"), "demo", "two");
    write_skill(&fixture.repo.join("skills/extra"), "extra", "two");
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "advance demo and extra"]);

    // A single name still selects exactly that one entry.
    let single = fixture
        .command()
        .args(["update", "demo", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&single);
    let changes = json(&single)["changes"].as_array().unwrap().clone();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["name"], "demo");
    assert_eq!(
        fs::read_to_string(fixture.project.join(".agents/skills/demo/data.txt")).unwrap(),
        "two"
    );
    assert_eq!(
        fs::read_to_string(fixture.project.join(".agents/skills/extra/data.txt")).unwrap(),
        "one"
    );

    // The bare form still reaches every remaining stale entry.
    let bare = fixture
        .command()
        .args(["update", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&bare);
    let changes = json(&bare)["changes"].as_array().unwrap().clone();
    let names: Vec<String> = changes
        .iter()
        .map(|change| change["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(names.contains(&"extra".to_owned()));
    assert_eq!(
        fs::read_to_string(fixture.project.join(".agents/skills/extra/data.txt")).unwrap(),
        "two"
    );
}

#[test]
fn json_envelope_for_multi_name_update_has_the_stable_shape() {
    let fixture = Fixture::new();
    fixture.init_add();
    add_extra_and_other(&fixture);
    write_skill(&fixture.repo.join("skills/demo"), "demo", "two");
    write_skill(&fixture.repo.join("skills/extra"), "extra", "two");
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "advance demo and extra"]);

    let output = fixture
        .command()
        .args(["update", "demo", "extra", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&output);
    let document = json(&output);
    assert_eq!(document["ok"], true);
    assert_eq!(document["scope"], "project");
    assert!(document["lock_file"]
        .as_str()
        .unwrap()
        .ends_with("skills.lock.yaml"));
    assert_eq!(document["skills"].as_array().unwrap().len(), 0);
    assert!(document["errors"].as_array().unwrap().is_empty());
    let changes = document["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    for change in changes {
        assert_eq!(change["action"], "update");
        assert_eq!(change["dry_run"], false);
        assert!(change["commit"].is_string());
        assert!(change["digest"].is_string());
    }
}

#[test]
fn sync_check_and_diff_accept_multiple_names() {
    let fixture = Fixture::new();
    fixture.init_add();
    add_extra_and_other(&fixture);

    let check_output = fixture
        .command()
        .args(["check", "demo", "extra", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&check_output);
    let skills = json(&check_output)["skills"].as_array().unwrap().clone();
    let names: Vec<String> = skills
        .iter()
        .map(|skill| skill["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, vec!["demo".to_owned(), "extra".to_owned()]);

    let diff_output = fixture
        .command()
        .args(["diff", "demo", "extra", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&diff_output);
    assert!(json(&diff_output)["changes"].as_array().unwrap().is_empty());

    // `other` was installed by `add` already; capture its identity so the
    // sync below, which never names it, can be shown to leave it alone.
    let other_before = file_identity(&fixture.project.join(".agents/skills/other/data.txt"));

    fs::remove_dir_all(fixture.project.join(".agents/skills/demo")).unwrap();
    fs::remove_dir_all(fixture.project.join(".agents/skills/extra")).unwrap();
    let sync_output = fixture
        .command()
        .args(["sync", "demo", "extra", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&sync_output);
    let changes = json(&sync_output)["changes"].as_array().unwrap().clone();
    let names: Vec<String> = changes
        .iter()
        .map(|change| change["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, vec!["demo".to_owned(), "extra".to_owned()]);
    assert!(fixture.project.join(".agents/skills/demo").exists());
    assert!(fixture.project.join(".agents/skills/extra").exists());
    // `other` was never named by any of the three commands above, so its
    // installed content and inode stay exactly as `add` left them.
    assert_eq!(
        file_identity(&fixture.project.join(".agents/skills/other/data.txt")),
        other_before
    );
}

#[test]
fn status_omits_the_combined_suggestion_for_zero_or_one_stale_entries() {
    let fixture = Fixture::new();
    fixture.init_add();
    // `other` is a genuinely distinct source, so advancing `demo` below
    // cannot also change `other`'s classification the way a shared-ref
    // entry would.
    add_other(&fixture);

    // Nothing stale yet: no combined suggestion line.
    let clean = fixture.command().arg("status").output().unwrap();
    assert_ok_ref(&clean);
    let clean_stdout = String::from_utf8(clean.stdout).unwrap();
    assert!(!clean_stdout.contains("run all:"));

    // Advance only `demo`: exactly one stale entry, so the combined line
    // still does not apply (it only fires once two or more entries qualify).
    fixture.advance("two");
    let one_stale = fixture.command().arg("status").output().unwrap();
    assert_ok_ref(&one_stale);
    let one_stale_stdout = String::from_utf8(one_stale.stdout).unwrap();
    assert!(one_stale_stdout.contains("update_available"));
    assert!(!one_stale_stdout.contains("run all:"));
}

#[test]
fn status_combines_update_available_and_pin_only_rows_into_one_suggestion() {
    let fixture = Fixture::new();
    fixture.init_add();
    // `extra` shares `demo`'s repository and ref, so advancing that shared
    // source while touching only `demo`'s content makes `demo` report
    // `update_available` and `extra` report the pin-only `source_advanced`,
    // the mix the combined line must cover.
    add_extra(&fixture);
    fixture.advance("two");

    let output = fixture.command().arg("status").output().unwrap();
    assert_ok_ref(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("update_available"));
    assert!(stdout.contains("source_advanced"));
    assert_eq!(stdout.matches("run all:").count(), 1);
    assert!(stdout.contains("run all: skillctl update demo extra"));

    // The `--json` document stays limited to the existing per-entry field;
    // each entry's own `recommended_action` keeps its distinct wording.
    let json_output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&json_output);
    let document = json(&json_output);
    let raw = serde_json::to_string(&document).unwrap();
    assert!(!raw.contains("run all"));
    let skills = document["skills"].as_array().unwrap();
    let demo = skills.iter().find(|skill| skill["name"] == "demo").unwrap();
    let extra = skills
        .iter()
        .find(|skill| skill["name"] == "extra")
        .unwrap();
    assert_eq!(demo["recommended_action"], "skillctl update demo");
    assert_eq!(
        extra["recommended_action"],
        "skillctl update extra (pin only)"
    );
}
