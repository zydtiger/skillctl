use crate::lockfile::safe_relative_path;
use anyhow::{bail, Context, Result};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

pub(super) fn clone_bare(repository: &str, bare: &Path) -> Result<()> {
    run_git(
        Command::new("git")
            .args([
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
                "clone",
            ])
            .arg("--bare")
            .arg("--quiet")
            .arg("--")
            .arg(repository)
            .arg(bare),
        "clone repository",
    )
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
