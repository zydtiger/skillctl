use crate::lockfile::safe_relative_path;
use anyhow::{bail, Context, Result};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

/// The transport allow-list every Git invocation that talks to a remote must
/// carry: only file, http(s), ssh, and git are permitted; the process-
/// executing `ext` transport (and anything unlisted) stays denied.
fn restricted_git() -> Command {
    let mut command = Command::new("git");
    command.args([
        "-c",
        "protocol.allow=never",
        "-c",
        "protocol.file.allow=always",
        "-c",
        "protocol.http.allow=always",
        "-c",
        "protocol.https.allow=always",
        "-c",
        "protocol.ssh.allow=always",
        "-c",
        "protocol.git.allow=always",
        "-c",
        "protocol.ext.allow=never",
    ]);
    command
}

pub(super) fn clone_bare(repository: &str, bare: &Path) -> Result<()> {
    run_git(
        restricted_git()
            .arg("clone")
            .arg("--bare")
            .arg("--quiet")
            .arg("--")
            .arg(repository)
            .arg(bare),
        "clone repository",
    )
}

/// Materialize a bare repository containing at least the commit reachable
/// through `reference` (or the remote default branch when `reference` is
/// `None`), preferring the smallest transfer that can resolve it.
///
/// Correctness never depends on server support for shallow or commit-
/// addressed fetches: each shape below gets exactly one shallow attempt, and
/// any failure there falls back directly to today's full, unrestricted bare
/// clone -- never through another narrower attempt -- so acquisition never
/// costs more than two Git invocations against the remote.
/// - No reference: a shallow clone of the remote default branch tip, else
///   the full clone.
/// - A branch- or tag-shaped reference: a shallow single-branch clone of
///   that ref, else the full clone.
/// - A revision-shaped reference (full or abbreviated hex): a commit-
///   addressed shallow fetch of the exact reference, else the full clone.
pub(super) fn acquire_bare(repository: &str, bare: &Path, reference: Option<&str>) -> Result<()> {
    let shallow_succeeded = match reference {
        None => clone_bare_shallow_default(repository, bare).is_ok(),
        Some(reference) if looks_revision_shaped(reference) => {
            fetch_bare_commit(repository, bare, reference).is_ok()
        }
        Some(reference) => clone_bare_shallow_branch(repository, bare, reference).is_ok(),
    };
    if shallow_succeeded {
        return Ok(());
    }
    remove_bare(bare);
    clone_bare(repository, bare)
}

/// Clone only the tip of the remote's default branch, transferring no other
/// branch and no history beyond that single commit.
///
/// `--no-local` is required for this to actually be shallow: Git's local-
/// clone fast path (used whenever `repository` is a plain filesystem path,
/// which is exactly what a local source normalizes to and persists in the
/// lock) otherwise ignores `--depth` entirely and hardlinks full history. It
/// is a no-op for a genuinely remote transport, where that fast path never
/// applies.
fn clone_bare_shallow_default(repository: &str, bare: &Path) -> Result<()> {
    run_git(
        restricted_git()
            .arg("clone")
            .arg("--bare")
            .arg("--quiet")
            .arg("--no-local")
            .arg("--depth")
            .arg("1")
            .arg("--")
            .arg(repository)
            .arg(bare),
        "shallow clone repository",
    )
}

/// Clone only the tip of a single named branch or tag, at depth one. Fails
/// for a revision that is not an exact branch or tag name, which is exactly
/// why the caller only attempts this for a reference that does not look
/// revision-shaped. See `clone_bare_shallow_default` for why `--no-local` is
/// required.
fn clone_bare_shallow_branch(repository: &str, bare: &Path, reference: &str) -> Result<()> {
    run_git(
        restricted_git()
            .arg("clone")
            .arg("--bare")
            .arg("--quiet")
            .arg("--no-local")
            .arg("--depth")
            .arg("1")
            .arg("--single-branch")
            .arg("--branch")
            .arg(reference)
            .arg("--")
            .arg(repository)
            .arg(bare),
        "shallow clone repository",
    )
}

/// Attempt a commit-addressed shallow fetch of exactly `reference` into a
/// freshly initialized bare repository. This fails when the server does not
/// advertise `reference` as a ref name and does not permit fetching it as a
/// bare commit (for example, a host that refuses a non-tip commit by
/// default) -- the caller treats that as ordinary fallback, not an error.
///
/// Forcing protocol version 0 makes that restriction observable through the
/// local file transport too: protocol version 2 resolves any object already
/// present in the source repository regardless of reachability, which would
/// otherwise make local fixtures unable to exercise the refusal path that
/// real hosts enforce.
fn fetch_bare_commit(repository: &str, bare: &Path, reference: &str) -> Result<()> {
    run_git(
        restricted_git()
            .arg("init")
            .arg("--quiet")
            .arg("--bare")
            .arg(bare),
        "initialize repository",
    )?;
    let mut fetch = restricted_git();
    fetch
        .arg("-c")
        .arg("protocol.version=0")
        .arg("-C")
        .arg(bare)
        .arg("fetch")
        .arg("--quiet")
        .arg("--depth")
        .arg("1")
        .arg("--")
        .arg(repository)
        .arg(reference);
    run_git(&mut fetch, "fetch commit")?;
    // A fetch by literal commit hash resolves directly against the object
    // database without any ref, but a hex-shaped branch or tag name needs a
    // local ref for `resolve_revision` to find it, the same way the
    // branch/tag clone path already provides one. `refs/heads/<reference>`
    // makes `reference` itself ambiguous with an object hash of the same
    // text when `reference` is hex-shaped, but that is benign: Git resolves
    // an unqualified name by trying it as a ref before an object hash, so a
    // hex-shaped branch or tag name still resolves to this ref, and a full
    // 40-character SHA still resolves to the identical commit either way.
    //
    // `FETCH_HEAD` is peeled to `^{commit}` because it may name an annotated
    // tag object rather than a commit -- `update-ref` refuses to point a
    // branch ref at a non-commit, and every other acquisition path already
    // resolves a tag down to the commit it points at.
    run_git(
        restricted_git()
            .arg("-C")
            .arg(bare)
            .args(["update-ref", "--create-reflog"])
            .arg(format!("refs/heads/{reference}"))
            .arg("FETCH_HEAD^{commit}"),
        "record fetched commit",
    )
}

fn remove_bare(bare: &Path) {
    let _ = fs::remove_dir_all(bare);
}

/// A followed reference is revision-shaped when it could be a full or
/// abbreviated commit hash, per the third case of the `--ref` contract:
/// entirely hexadecimal and within Git's usual abbreviation range. Such a
/// reference skips the branch/tag clone attempt, which cannot address a raw
/// revision, and goes straight to the commit-addressed fetch. A reference
/// that merely looks hex but actually names a branch or tag still resolves
/// correctly there, since Git matches an advertised ref name before falling
/// back to interpreting the text as a literal object hash.
fn looks_revision_shaped(reference: &str) -> bool {
    (4..=40).contains(&reference.len()) && reference.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn default_branch(bare: &Path) -> Result<String> {
    let output = git_output(
        Command::new("git")
            .arg("-C")
            .arg(bare)
            .args(["symbolic-ref", "HEAD"]),
        "discover default branch",
    )?;
    let full = String::from_utf8(output.stdout)?.trim().to_owned();
    full.strip_prefix("refs/heads/")
        .map(str::to_owned)
        .filter(|name| !name.is_empty())
        .context("repository default branch could not be reliably discovered; pass --ref")
}

pub(super) fn resolve_revision(bare: &Path, reference: &str) -> Result<String> {
    let candidates = [
        reference.to_owned(),
        format!("refs/heads/{reference}"),
        format!("refs/tags/{reference}"),
    ];
    for candidate in candidates {
        let output = Command::new("git")
            .arg("-C")
            .arg(bare)
            .args(["rev-parse", "--verify"])
            .arg(format!("{candidate}^{{commit}}"))
            .output()?;
        if output.status.success() {
            let commit = String::from_utf8(output.stdout)?.trim().to_owned();
            if commit.len() == 40 && commit.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Ok(commit);
            }
        }
    }
    bail!("could not resolve Git ref `{reference}` to a commit")
}

pub(super) fn export_file(
    bare: &Path,
    commit: &str,
    source_file: &Path,
    destination: &Path,
) -> Result<()> {
    let source = source_file
        .to_str()
        .context("source file is not valid UTF-8")?;
    let output = git_output(
        Command::new("git")
            .arg("-C")
            .arg(bare)
            .args(["ls-tree", "-z"])
            .arg(commit)
            .arg("--")
            .arg(source),
        "locate source file",
    )?;
    let records: Vec<&[u8]> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect();
    if records.len() != 1 {
        bail!("source file `{source}` is missing at commit {commit}");
    }
    let (mode, kind, object, full_path) = parse_tree_record(records[0])?;
    if full_path != source {
        bail!("Git returned unexpected source file `{full_path}` for `{source}`");
    }
    if kind != "blob" || (mode != "100644" && mode != "100755") {
        bail!("source file `{source}` is not an ordinary file");
    }
    write_blob(
        bare,
        object,
        &destination.join("SKILL.md"),
        mode == "100755",
    )
}

pub(super) fn export_tree(
    bare: &Path,
    commit: &str,
    source_path: &Path,
    destination: &Path,
) -> Result<()> {
    let source = source_path
        .to_str()
        .context("source path is not valid UTF-8")?;
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(bare)
        .args(["ls-tree", "-r", "-z"])
        .arg(commit)
        .arg("--");
    if !source.is_empty() {
        command.arg(source);
    }
    let output = git_output(&mut command, "list source tree")?;
    let prefix = if source.is_empty() {
        None
    } else {
        Some(format!("{source}/"))
    };
    let mut count = 0;
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
    {
        let (mode, kind, object, full_path) = parse_tree_record(raw)?;
        let relative = if let Some(prefix) = &prefix {
            full_path
                .strip_prefix(prefix)
                .with_context(|| format!("unexpected Git path `{full_path}` outside source tree"))?
        } else {
            full_path
        };
        let relative = safe_relative_path(relative)?;
        if kind != "blob" || (mode != "100644" && mode != "100755") {
            bail!("source tree contains unsupported {kind} with mode {mode}: {full_path}");
        }
        let target = destination.join(&relative);
        write_blob(bare, object, &target, mode == "100755")?;
        count += 1;
    }
    if count == 0 {
        let display = if source.is_empty() { "." } else { source };
        bail!("source path `{display}` is missing or contains no files at commit {commit}");
    }
    Ok(())
}

fn parse_tree_record(raw: &[u8]) -> Result<(&str, &str, &str, &str)> {
    let tab = raw
        .iter()
        .position(|byte| *byte == b'\t')
        .context("malformed git tree record")?;
    let header = std::str::from_utf8(&raw[..tab]).context("malformed git tree header")?;
    let full_path = std::str::from_utf8(&raw[tab + 1..]).context("Git path is not valid UTF-8")?;
    let mut fields = header.split_whitespace();
    let mode = fields.next().context("Git tree record is missing mode")?;
    let kind = fields.next().context("Git tree record is missing type")?;
    let object = fields.next().context("Git tree record is missing object")?;
    Ok((mode, kind, object, full_path))
}

fn write_blob(bare: &Path, object: &str, target: &Path, executable: bool) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let blob = git_output(
        Command::new("git")
            .arg("-C")
            .arg(bare)
            .args(["cat-file", "blob", object]),
        "read source blob",
    )?;
    let mut file = fs::File::create(target)?;
    file.write_all(&blob.stdout)?;
    set_executable(target, executable)
}

fn git_output(command: &mut Command, operation: &str) -> Result<Output> {
    command.env("GIT_TERMINAL_PROMPT", "0");
    let output = command
        .output()
        .with_context(|| format!("could not run git to {operation}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        bail!("git could not {operation}: {stderr}");
    }
    Ok(output)
}

fn run_git(command: &mut Command, operation: &str) -> Result<()> {
    git_output(command, operation).map(|_| ())
}

fn set_executable(path: &Path, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn git_test(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_test_stdout(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    /// A source repository with a `main` branch carrying several commits, a
    /// diverging `feature` branch, and a tag, so a shallow single-branch
    /// acquisition can be told apart from a full clone by ref count and
    /// history depth.
    fn build_source(dir: &Path) {
        git_test(dir, &["init", "-q", "-b", "main"]);
        git_test(dir, &["config", "user.email", "tests@example.com"]);
        git_test(dir, &["config", "user.name", "Tests"]);
        for content in ["one", "two", "three"] {
            fs::write(dir.join("file.txt"), content).unwrap();
            git_test(dir, &["add", "."]);
            git_test(dir, &["commit", "-qm", content]);
            if content == "two" {
                // Tag an earlier commit, not the branch tips, so a
                // single-branch shallow clone of `feature` below does not
                // incidentally also pick up this tag.
                git_test(dir, &["tag", "v1.0"]);
            }
        }
        git_test(dir, &["checkout", "-qb", "feature"]);
        fs::write(dir.join("feature.txt"), "feature").unwrap();
        git_test(dir, &["add", "."]);
        git_test(dir, &["commit", "-qm", "feature commit"]);
        git_test(dir, &["checkout", "-q", "main"]);
    }

    /// Using the `file://` transport (rather than a bare filesystem path)
    /// makes Git honor `--depth` for local sources too; a plain path silently
    /// ignores it for `clone` while still working through Git's file
    /// protocol, which is what production callers and other fixtures use.
    fn file_url(dir: &Path) -> String {
        format!("file://{}", dir.display())
    }

    fn ref_names(bare: &Path) -> String {
        git_test_stdout(bare, &["for-each-ref", "--format=%(refname)"])
    }

    fn commit_count(bare: &Path, revision: &str) -> usize {
        git_test_stdout(bare, &["log", "--oneline", revision])
            .lines()
            .filter(|line| !line.is_empty())
            .count()
    }

    #[test]
    fn no_reference_transfers_only_the_default_branch_tip() {
        let source = tempdir().unwrap();
        build_source(source.path());
        let main_tip = git_test_stdout(source.path(), &["rev-parse", "main"]);

        let staging = tempdir().unwrap();
        let bare = staging.path().join("repo.git");
        acquire_bare(&file_url(source.path()), &bare, None).unwrap();

        assert!(bare.join("shallow").is_file(), "expected a shallow clone");
        assert_eq!(ref_names(&bare), "refs/heads/main");
        assert_eq!(commit_count(&bare, "HEAD"), 1);
        assert_eq!(default_branch(&bare).unwrap(), "main");
        assert_eq!(resolve_revision(&bare, "main").unwrap(), main_tip);
    }

    #[test]
    fn branch_reference_transfers_only_that_branch_at_depth_one() {
        let source = tempdir().unwrap();
        build_source(source.path());
        let feature_tip = git_test_stdout(source.path(), &["rev-parse", "feature"]);

        let staging = tempdir().unwrap();
        let bare = staging.path().join("repo.git");
        acquire_bare(&file_url(source.path()), &bare, Some("feature")).unwrap();

        assert!(bare.join("shallow").is_file(), "expected a shallow clone");
        assert_eq!(ref_names(&bare), "refs/heads/feature");
        assert_eq!(commit_count(&bare, "refs/heads/feature"), 1);
        assert_eq!(resolve_revision(&bare, "feature").unwrap(), feature_tip);
    }

    #[test]
    fn tag_reference_transfers_only_that_tag_at_depth_one() {
        let source = tempdir().unwrap();
        build_source(source.path());
        let tag_tip = git_test_stdout(source.path(), &["rev-parse", "v1.0"]);

        let staging = tempdir().unwrap();
        let bare = staging.path().join("repo.git");
        acquire_bare(&file_url(source.path()), &bare, Some("v1.0")).unwrap();

        assert!(bare.join("shallow").is_file(), "expected a shallow clone");
        assert_eq!(ref_names(&bare), "refs/tags/v1.0");
        assert_eq!(resolve_revision(&bare, "v1.0").unwrap(), tag_tip);
    }

    #[test]
    fn raw_commit_hash_resolves_via_commit_addressed_fetch_when_the_server_allows_it() {
        let source = tempdir().unwrap();
        build_source(source.path());
        // The root commit on main is reachable but not a ref tip, which is
        // exactly the case a host must opt into serving.
        let pinned = git_test_stdout(source.path(), &["rev-list", "--max-parents=0", "main"]);
        git_test(
            source.path(),
            &["config", "uploadpack.allowReachableSHA1InWant", "true"],
        );

        let staging = tempdir().unwrap();
        let bare = staging.path().join("repo.git");
        acquire_bare(&file_url(source.path()), &bare, Some(&pinned)).unwrap();

        assert!(
            bare.join("shallow").is_file(),
            "expected a commit-addressed shallow fetch, not a full clone"
        );
        assert_eq!(resolve_revision(&bare, &pinned).unwrap(), pinned);
    }

    /// `normalize_repository` in the `add` command canonicalizes a local
    /// source to a plain filesystem path (no `file://` scheme) and persists
    /// that shape in the lock, so this is what production acquisition
    /// actually runs against for every local source -- unlike the other
    /// tests in this module, which use `file://` specifically to make Git
    /// honor `--depth`. Without `--no-local` on the shallow clone attempts,
    /// this path silently produced a full local hardlink clone instead.
    #[test]
    fn no_reference_transfers_only_the_default_branch_tip_from_a_plain_local_path() {
        let source = tempdir().unwrap();
        build_source(source.path());
        let main_tip = git_test_stdout(source.path(), &["rev-parse", "main"]);

        let staging = tempdir().unwrap();
        let bare = staging.path().join("repo.git");
        acquire_bare(source.path().to_str().unwrap(), &bare, None).unwrap();

        assert!(
            bare.join("shallow").is_file(),
            "expected a shallow clone from a plain local path"
        );
        assert_eq!(resolve_revision(&bare, "main").unwrap(), main_tip);
    }

    /// See `no_reference_transfers_only_the_default_branch_tip_from_a_plain_local_path`
    /// for why the plain-path shape matters; this covers the same
    /// requirement for the single-branch clone attempt.
    #[test]
    fn branch_reference_transfers_only_that_branch_at_depth_one_from_a_plain_local_path() {
        let source = tempdir().unwrap();
        build_source(source.path());
        let feature_tip = git_test_stdout(source.path(), &["rev-parse", "feature"]);

        let staging = tempdir().unwrap();
        let bare = staging.path().join("repo.git");
        acquire_bare(source.path().to_str().unwrap(), &bare, Some("feature")).unwrap();

        assert!(
            bare.join("shallow").is_file(),
            "expected a shallow clone from a plain local path"
        );
        assert_eq!(resolve_revision(&bare, "feature").unwrap(), feature_tip);
    }

    /// An ordinary (non-hex) annotated tag name goes through
    /// `clone_bare_shallow_branch`, not the commit-addressed fetch path; the
    /// pinned commit `resolve_revision` returns must be the commit the tag
    /// points at, not the tag object itself.
    #[test]
    fn ordinary_named_annotated_tag_transfers_only_that_tag_at_depth_one() {
        let source = tempdir().unwrap();
        build_source(source.path());
        git_test(
            source.path(),
            &["tag", "-a", "stable-release", "-m", "annotated", "main"],
        );
        let tag_commit = git_test_stdout(source.path(), &["rev-parse", "main"]);

        let staging = tempdir().unwrap();
        let bare = staging.path().join("repo.git");
        acquire_bare(&file_url(source.path()), &bare, Some("stable-release")).unwrap();

        assert!(bare.join("shallow").is_file(), "expected a shallow clone");
        assert_eq!(ref_names(&bare), "refs/tags/stable-release");
        assert_eq!(
            resolve_revision(&bare, "stable-release").unwrap(),
            tag_commit
        );
    }

    #[test]
    fn hex_shaped_branch_name_resolves_via_commit_addressed_fetch_at_depth_one() {
        let source = tempdir().unwrap();
        build_source(source.path());
        git_test(source.path(), &["branch", "deadbeef", "main"]);
        let branch_commit = git_test_stdout(source.path(), &["rev-parse", "deadbeef"]);

        let staging = tempdir().unwrap();
        let bare = staging.path().join("repo.git");
        acquire_bare(&file_url(source.path()), &bare, Some("deadbeef")).unwrap();

        assert!(
            bare.join("shallow").is_file(),
            "expected a shallow commit-addressed fetch, not a full clone"
        );
        assert_eq!(resolve_revision(&bare, "deadbeef").unwrap(), branch_commit);
    }

    #[test]
    fn hex_shaped_lightweight_tag_resolves_via_commit_addressed_fetch_at_depth_one() {
        let source = tempdir().unwrap();
        build_source(source.path());
        git_test(source.path(), &["tag", "cafebabe", "main"]);
        let tag_commit = git_test_stdout(source.path(), &["rev-parse", "main"]);

        let staging = tempdir().unwrap();
        let bare = staging.path().join("repo.git");
        acquire_bare(&file_url(source.path()), &bare, Some("cafebabe")).unwrap();

        assert!(
            bare.join("shallow").is_file(),
            "expected a shallow commit-addressed fetch, not a full clone"
        );
        assert_eq!(resolve_revision(&bare, "cafebabe").unwrap(), tag_commit);
    }

    /// An annotated tag's `FETCH_HEAD` names the tag object, not the commit
    /// it points at; without peeling to `^{commit}` before `update-ref`, this
    /// acquisition step failed outright and silently fell back to a full
    /// clone, defeating the shallow transfer for every hex- or numeric-named
    /// annotated tag.
    #[test]
    fn annotated_hex_named_tag_resolves_via_commit_addressed_fetch_at_depth_one() {
        let source = tempdir().unwrap();
        build_source(source.path());
        git_test(
            source.path(),
            &["tag", "-a", "cafe1234", "-m", "annotated", "main"],
        );
        let tag_commit = git_test_stdout(source.path(), &["rev-parse", "main"]);

        let staging = tempdir().unwrap();
        let bare = staging.path().join("repo.git");
        acquire_bare(&file_url(source.path()), &bare, Some("cafe1234")).unwrap();

        assert!(
            bare.join("shallow").is_file(),
            "expected a shallow commit-addressed fetch, not a full clone"
        );
        assert_eq!(resolve_revision(&bare, "cafe1234").unwrap(), tag_commit);
    }

    #[test]
    fn refused_commit_addressed_fetch_falls_back_to_a_full_clone() {
        let source = tempdir().unwrap();
        build_source(source.path());
        let pinned = git_test_stdout(source.path(), &["rev-list", "--max-parents=0", "main"]);
        // Left at the default (unset), a non-tip commit fetch is refused.

        let staging = tempdir().unwrap();
        let bare = staging.path().join("repo.git");
        acquire_bare(&file_url(source.path()), &bare, Some(&pinned)).unwrap();

        assert!(
            !bare.join("shallow").is_file(),
            "expected the full-clone fallback, not a shallow repository"
        );
        assert!(commit_count(&bare, "refs/heads/main") >= 3);
        assert_eq!(resolve_revision(&bare, &pinned).unwrap(), pinned);
    }
}
