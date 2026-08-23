//! Coverage for the shallow, single-branch, and commit-addressed source
//! acquisition strategy in `src/source/git.rs`: `status`, `update`, `sync`,
//! and `diff` must resolve, install, and report identically to a full clone
//! regardless of which acquisition path served the pinned or followed
//! reference, including a followed reference that is a raw commit hash and
//! the fallback route when a commit-addressed fetch is refused.

mod support;

use support::*;

/// A followed raw commit hash that is an ancestor of the branch tip, not the
/// tip itself, is exactly the case a commit-addressed shallow fetch needs
/// server support for; left unconfigured, the fixture refuses it and the
/// acquisition falls back to a full clone. `status`, `sync`, `diff`, and
/// `update` must all still resolve, install, and report the pinned commit
/// and digest correctly through that fallback.
#[test]
fn raw_commit_hash_reference_resolves_through_the_full_clone_fallback() {
    let fixture = Fixture::new();
    let pinned_commit = git_stdout(&fixture.repo, &["rev-parse", "HEAD"]);
    // Advance the source twice so the pinned commit is a non-tip ancestor
    // with real history beyond it.
    fixture.advance("two");
    fixture.advance("three");

    assert_ok(fixture.command().arg("init").output().unwrap());
    let add = fixture
        .command()
        .args([
            "add",
            fixture.repo.to_str().unwrap(),
            "--path",
            "skills/demo",
            "--ref",
            &pinned_commit,
            "--json",
        ])
        .output()
        .unwrap();
    assert_ok_ref(&add);
    let added = json(&add);
    assert_eq!(added["skills"][0]["commit"], pinned_commit);
    let pinned_digest = added["skills"][0]["digest"].as_str().unwrap().to_owned();

    let status = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&status);
    let skill = &json(&status)["skills"][0];
    assert_eq!(skill["pinned_commit"], pinned_commit);
    assert_eq!(skill["upstream_commit"], pinned_commit);
    assert_eq!(skill["upstream_status"], "current");
    assert_eq!(skill["content_changed"], false);

    assert_ok(fixture.command().arg("check").output().unwrap());

    let diff = fixture.command().args(["diff", "--json"]).output().unwrap();
    assert_ok_ref(&diff);
    assert_eq!(json(&diff)["changes"].as_array().unwrap().len(), 0);

    let sync = fixture.command().args(["sync", "--json"]).output().unwrap();
    assert_ok_ref(&sync);
    assert_eq!(json(&sync)["changes"][0]["commit"], pinned_commit);

    // The followed reference is a fixed commit hash, so a later `update`
    // resolves the very same commit again: a no-op, not an advance.
    let update = fixture
        .command()
        .args(["update", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&update);
    assert_eq!(json(&update)["changes"][0]["action"], "no-op");
    assert_eq!(json(&update)["changes"][0]["commit"], pinned_commit);

    let list = fixture.command().args(["list", "--json"]).output().unwrap();
    assert_ok_ref(&list);
    let listed = json(&list);
    assert_eq!(listed["skills"][0]["commit"], pinned_commit);
    assert_eq!(listed["skills"][0]["digest"], pinned_digest);
}

/// The same non-tip commit reference, but with the fixture repository
/// explicitly configured to allow it, exercises the direct commit-addressed
/// shallow fetch instead of the full-clone fallback. It must resolve to the
/// identical commit and digest as the fallback path above.
#[test]
fn raw_commit_hash_reference_resolves_through_a_supported_commit_addressed_fetch() {
    let fixture = Fixture::new();
    let pinned_commit = git_stdout(&fixture.repo, &["rev-parse", "HEAD"]);
    fixture.advance("two");
    fixture.advance("three");
    git(
        &fixture.repo,
        &["config", "uploadpack.allowReachableSHA1InWant", "true"],
    );

    assert_ok(fixture.command().arg("init").output().unwrap());
    let add = fixture
        .command()
        .args([
            "add",
            fixture.repo.to_str().unwrap(),
            "--path",
            "skills/demo",
            "--ref",
            &pinned_commit,
            "--json",
        ])
        .output()
        .unwrap();
    assert_ok_ref(&add);
    let added = json(&add);
    assert_eq!(added["skills"][0]["commit"], pinned_commit);
    let pinned_digest = added["skills"][0]["digest"].as_str().unwrap().to_owned();

    let status = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&status);
    let skill = &json(&status)["skills"][0];
    assert_eq!(skill["pinned_commit"], pinned_commit);
    assert_eq!(skill["upstream_status"], "current");

    let diff = fixture.command().args(["diff", "--json"]).output().unwrap();
    assert_ok_ref(&diff);
    assert_eq!(json(&diff)["changes"].as_array().unwrap().len(), 0);

    let sync = fixture.command().args(["sync", "--json"]).output().unwrap();
    assert_ok_ref(&sync);
    assert_eq!(json(&sync)["changes"][0]["commit"], pinned_commit);

    // The commit-addressed shallow fetch must export the identical tree the
    // full-clone fallback does for the same pinned commit: same commit, same
    // digest, whichever acquisition path served it.
    let list = fixture.command().args(["list", "--json"]).output().unwrap();
    assert_ok_ref(&list);
    let listed = json(&list);
    assert_eq!(listed["skills"][0]["commit"], pinned_commit);
    assert_eq!(listed["skills"][0]["digest"], pinned_digest);
}

/// A followed branch reference against a fixture repository with multi-
/// commit history exercises the shallow single-branch clone path end to
/// end; `status`, `update`, `diff`, and `sync` must all still see and apply
/// an upstream advance exactly as a full clone would.
#[test]
fn followed_branch_reference_sees_upstream_advances_with_multi_commit_history() {
    let fixture = Fixture::new();
    fixture.init_add();
    let advanced_commit = fixture.advance("two");
    fixture.advance("three");
    let latest_commit = fixture.advance("four");

    let status = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&status);
    let skill = &json(&status)["skills"][0];
    assert_eq!(skill["upstream_commit"], latest_commit);
    assert_eq!(skill["upstream_status"], "update_available");
    assert_ne!(advanced_commit, latest_commit);

    let update = fixture
        .command()
        .args(["update", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&update);
    assert_eq!(json(&update)["changes"][0]["action"], "update");
    assert_eq!(json(&update)["changes"][0]["commit"], latest_commit);

    assert_ok(fixture.command().arg("check").output().unwrap());
    let diff = fixture.command().args(["diff", "--json"]).output().unwrap();
    assert_ok_ref(&diff);
    assert_eq!(json(&diff)["changes"].as_array().unwrap().len(), 0);

    let sync = fixture.command().args(["sync", "--json"]).output().unwrap();
    assert_ok_ref(&sync);
    assert_eq!(json(&sync)["changes"][0]["commit"], latest_commit);
}

/// An abbreviated commit hash can never be resolved by a server-side
/// commit-addressed fetch -- `git fetch <repo> <abbreviation>` fails to
/// match any advertised ref regardless of server permission -- so this
/// followed reference always pays for a doomed shallow attempt before
/// falling back to a full clone. That fallback must still resolve, pin, and
/// digest the correct commit.
#[test]
fn abbreviated_commit_hash_reference_resolves_through_the_full_clone_fallback() {
    let fixture = Fixture::new();
    let full_commit = git_stdout(&fixture.repo, &["rev-parse", "HEAD"]);
    let abbreviated = &full_commit[..8];
    fixture.advance("two");
    fixture.advance("three");

    assert_ok(fixture.command().arg("init").output().unwrap());
    let add = fixture
        .command()
        .args([
            "add",
            fixture.repo.to_str().unwrap(),
            "--path",
            "skills/demo",
            "--ref",
            abbreviated,
            "--json",
        ])
        .output()
        .unwrap();
    assert_ok_ref(&add);
    let added = json(&add);
    assert_eq!(added["skills"][0]["commit"], full_commit);
    let pinned_digest = added["skills"][0]["digest"].as_str().unwrap().to_owned();

    let status = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_ok_ref(&status);
    let skill = &json(&status)["skills"][0];
    assert_eq!(skill["pinned_commit"], full_commit);
    assert_eq!(skill["upstream_commit"], full_commit);
    assert_eq!(skill["upstream_status"], "current");

    let sync = fixture.command().args(["sync", "--json"]).output().unwrap();
    assert_ok_ref(&sync);
    assert_eq!(json(&sync)["changes"][0]["commit"], full_commit);

    let list = fixture.command().args(["list", "--json"]).output().unwrap();
    assert_ok_ref(&list);
    let listed = json(&list);
    assert_eq!(listed["skills"][0]["commit"], full_commit);
    assert_eq!(listed["skills"][0]["digest"], pinned_digest);
}
