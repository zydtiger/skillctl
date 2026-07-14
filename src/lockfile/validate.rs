use anyhow::{bail, Context, Result};
use std::path::{Component, Path, PathBuf};

pub fn safe_source_path(value: &str) -> Result<PathBuf> {
    if value == "." {
        Ok(PathBuf::new())
    } else {
        safe_relative_path(value)
    }
}

pub fn safe_source_file(value: &str) -> Result<PathBuf> {
    let path = safe_relative_path(value)?;
    if path.file_name().and_then(|name| name.to_str()) != Some("SKILL.md") {
        bail!("source file must be named SKILL.md");
    }
    Ok(path)
}

pub fn safe_relative_path(value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.contains('\0') || value.contains('\\') {
        bail!("path must be a nonempty POSIX-style relative path");
    }
    let path = Path::new(value);
    if path.is_absolute() {
        bail!("absolute paths are not allowed");
    }
    let mut count = 0;
    for component in path.components() {
        match component {
            Component::Normal(part) if !part.is_empty() => {
                let part = part.to_str().context("path is not valid UTF-8")?;
                if part.chars().any(char::is_control) {
                    bail!("control characters are not allowed in paths");
                }
                count += 1;
            }
            _ => bail!("dot, empty, root, prefix, and parent path components are not allowed"),
        }
    }
    if count == 0 || value.split('/').any(str::is_empty) {
        bail!("empty path components are not allowed");
    }
    Ok(path.to_path_buf())
}

pub fn validate_identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('-')
        || value.ends_with('-')
        || value.split('-').any(str::is_empty)
        || !value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        bail!("must be 1-64 lowercase letters, digits, or hyphens without edge hyphens");
    }
    Ok(())
}

pub fn validate_repository(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') || value.chars().any(char::is_control) {
        bail!("repository must be nonempty and contain no control characters");
    }
    Ok(())
}

pub fn validate_git_reference(value: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.starts_with('-')
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        bail!("ref must be nonempty, must not start with '-', and contain no control characters");
    }
    Ok(())
}

pub(crate) fn validate_commit(value: &str) -> Result<()> {
    if value.len() != 40 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("must be a 40-character hexadecimal Git commit");
    }
    Ok(())
}

pub(crate) fn validate_digest(value: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        bail!("must start with sha256:");
    };
    if hex.len() != 64 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("must contain a 64-character hexadecimal SHA-256");
    }
    Ok(())
}
