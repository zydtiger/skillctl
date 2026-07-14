use crate::digest::digest_tree;
use crate::lockfile::{
    safe_relative_path, safe_source_path, validate_git_reference, validate_identifier,
    validate_repository,
};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

#[derive(Debug)]
pub struct Snapshot {
    _temp: TempDir,
    pub root: PathBuf,
    pub commit: String,
    pub reference: String,
    pub digest: String,
    pub metadata: SkillMetadata,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
}

pub fn acquire(repository: &str, source_path: &str, reference: Option<&str>) -> Result<Snapshot> {
    validate_repository(repository)?;
    let source_path = safe_source_path(source_path)?;
    let temp = tempfile::tempdir().context("could not create source staging directory")?;
    let bare = temp.path().join("repository.git");
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
            .arg(&bare),
        "clone repository",
    )?;

    let followed = match reference {
        Some(value) if !value.trim().is_empty() => value.to_owned(),
        Some(_) => bail!("ref must not be empty"),
        None => default_branch(&bare)?,
    };
    validate_git_reference(&followed)?;
    let commit = resolve_revision(&bare, &followed)?;
    let root = temp.path().join("snapshot");
    fs::create_dir(&root)?;
    export_tree(&bare, &commit, &source_path, &root)?;
    if root.join(crate::digest::MARKER_FILE).exists() {
        bail!(
            "source tree must not contain reserved file {}",
            crate::digest::MARKER_FILE
        );
    }
    let metadata = validate_skill_tree(&root)?;
    let digest = digest_tree(&root)?;
    Ok(Snapshot {
        _temp: temp,
        root,
        commit,
        reference: followed,
        digest,
        metadata,
    })
}

pub fn validate_skill_tree(root: &Path) -> Result<SkillMetadata> {
    crate::digest::inspect_tree(root)?;
    let skill_path = root.join("SKILL.md");
    let content = fs::read_to_string(&skill_path).with_context(|| {
        format!(
            "skill requires UTF-8 root SKILL.md at {}",
            skill_path.display()
        )
    })?;
    let frontmatter = parse_frontmatter(&content)?;
    let metadata: SkillMetadata = serde_yaml_ng::from_str(frontmatter)
        .context("SKILL.md frontmatter must contain only name and description")?;
    validate_identifier(&metadata.name).context("invalid declared skill name")?;
    if metadata.description.trim().is_empty() {
        bail!("SKILL.md description must not be empty");
    }
    Ok(metadata)
}

fn parse_frontmatter(content: &str) -> Result<&str> {
    let normalized = content.strip_prefix('\u{feff}').unwrap_or(content);
    let rest = normalized
        .strip_prefix("---\n")
        .or_else(|| normalized.strip_prefix("---\r\n"))
        .context("SKILL.md must begin with YAML frontmatter delimited by ---")?;
    let end = rest
        .find("\n---\n")
        .or_else(|| rest.find("\r\n---\r\n"))
        .context("SKILL.md frontmatter is missing its closing ---")?;
    Ok(&rest[..end])
}

fn default_branch(bare: &Path) -> Result<String> {
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

fn resolve_revision(bare: &Path, reference: &str) -> Result<String> {
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

fn export_tree(bare: &Path, commit: &str, source_path: &Path, destination: &Path) -> Result<()> {
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
        let tab = raw
            .iter()
            .position(|byte| *byte == b'\t')
            .context("malformed git tree record")?;
        let header = std::str::from_utf8(&raw[..tab]).context("malformed git tree header")?;
        let full_path =
            std::str::from_utf8(&raw[tab + 1..]).context("Git path is not valid UTF-8")?;
        let relative = if let Some(prefix) = &prefix {
            full_path
                .strip_prefix(prefix)
                .with_context(|| format!("unexpected Git path `{full_path}` outside source tree"))?
        } else {
            full_path
        };
        let relative = safe_relative_path(relative)?;
        let mut fields = header.split_whitespace();
        let mode = fields.next().context("Git tree record is missing mode")?;
        let kind = fields.next().context("Git tree record is missing type")?;
        let object = fields.next().context("Git tree record is missing object")?;
        if kind != "blob" || (mode != "100644" && mode != "100755") {
            bail!("source tree contains unsupported {kind} with mode {mode}: {full_path}");
        }
        let target = destination.join(&relative);
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
        let mut file = fs::File::create(&target)?;
        file.write_all(&blob.stdout)?;
        set_executable(&target, mode == "100755")?;
        count += 1;
    }
    if count == 0 {
        let display = if source.is_empty() { "." } else { source };
        bail!("source path `{display}` is missing or contains no files at commit {commit}");
    }
    Ok(())
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

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _executable: bool) -> Result<()> {
    Ok(())
}
