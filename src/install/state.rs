use super::{marker_for, read_marker, validate_destination_location};
use crate::digest::digest_tree;
use crate::lockfile::{Mode, SkillEntry};
use crate::scope::Scope;
use crate::source::{validate_skill_tree, SkillMetadata};
use serde_json::json;

#[derive(Clone, Debug)]
pub struct EntryState {
    pub state: String,
    pub details: Vec<String>,
    pub metadata: Option<SkillMetadata>,
}

pub fn entry_state(scope: &Scope, name: &str, entry: &SkillEntry) -> EntryState {
    let destination = scope.skills_dir.join(&entry.destination);
    if let Err(error) = validate_destination_location(scope, entry) {
        return EntryState {
            state: "invalid".to_owned(),
            details: vec![format!("{error:#}")],
            metadata: None,
        };
    }
    if !destination.exists() {
        return EntryState {
            state: "missing".to_owned(),
            details: vec![format!("{} does not exist", destination.display())],
            metadata: None,
        };
    }
    let metadata = match validate_skill_tree(&destination) {
        Ok(metadata) => metadata,
        Err(error) => {
            return EntryState {
                state: "invalid".to_owned(),
                details: vec![format!("{error:#}")],
                metadata: None,
            };
        }
    };
    if entry.mode == Mode::Local {
        return EntryState {
            state: "local".to_owned(),
            details: Vec::new(),
            metadata: Some(metadata),
        };
    }
    let expected_marker = match marker_for(scope, name, entry) {
        Ok(marker) => marker,
        Err(error) => {
            return EntryState {
                state: "invalid".to_owned(),
                details: vec![format!("{error:#}")],
                metadata: Some(metadata),
            };
        }
    };
    let marker = match read_marker(&destination) {
        Ok(marker) => marker,
        Err(error) => {
            return EntryState {
                state: "invalid".to_owned(),
                details: vec![format!("{error:#}")],
                metadata: Some(metadata),
            };
        }
    };
    if marker != expected_marker {
        return EntryState {
            state: "invalid".to_owned(),
            details: vec!["managed marker does not match the lock entry".to_owned()],
            metadata: Some(metadata),
        };
    }
    let actual = match digest_tree(&destination) {
        Ok(digest) => digest,
        Err(error) => {
            return EntryState {
                state: "invalid".to_owned(),
                details: vec![format!("{error:#}")],
                metadata: Some(metadata),
            };
        }
    };
    let expected = &entry.resolved.as_ref().expect("validated lock").digest;
    if &actual == expected {
        EntryState {
            state: "clean".to_owned(),
            details: Vec::new(),
            metadata: Some(metadata),
        }
    } else {
        EntryState {
            state: "modified".to_owned(),
            details: vec![format!("digest is {actual}, expected {expected}")],
            metadata: Some(metadata),
        }
    }
}

pub fn skill_json(name: &str, entry: &SkillEntry, state: Option<&EntryState>) -> serde_json::Value {
    let source = entry.source.as_ref();
    let resolved = entry.resolved.as_ref();
    json!({
        "name": name,
        "mode": match entry.mode { Mode::Vendored => "vendored", Mode::Local => "local" },
        "destination": entry.destination,
        "source": source.map(|source| json!({
            "repository": source.repository,
            "path": source.path,
            "file": source.file,
            "ref": source.reference,
        })),
        "commit": resolved.map(|resolved| resolved.commit.clone()),
        "digest": resolved.map(|resolved| resolved.digest.clone()),
        "state": state.map(|state| state.state.clone()),
        "details": state.map(|state| state.details.clone()).unwrap_or_default(),
        "declared_name": state.and_then(|state| state.metadata.as_ref()).map(|meta| meta.name.clone()),
    })
}
