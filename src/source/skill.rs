use crate::lockfile::validate_identifier;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
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
