use crate::scope::Scope;
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};

#[derive(Debug, Serialize)]
pub struct Envelope {
    pub ok: bool,
    pub scope: String,
    pub lock_file: String,
    pub skills: Vec<Value>,
    pub changes: Vec<Value>,
    pub errors: Vec<String>,
}

impl Envelope {
    pub fn new(scope: &Scope) -> Self {
        Self {
            ok: true,
            scope: scope.label().to_owned(),
            lock_file: scope.lock_file.display().to_string(),
            skills: Vec::new(),
            changes: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn failure(scope: Option<&Scope>, global: bool, error: &anyhow::Error) -> Self {
        let (scope_name, lock_file) = if let Some(scope) = scope {
            (
                scope.label().to_owned(),
                scope.lock_file.display().to_string(),
            )
        } else {
            (
                if global { "global" } else { "project" }.to_owned(),
                String::new(),
            )
        };
        Self {
            ok: false,
            scope: scope_name,
            lock_file,
            skills: Vec::new(),
            changes: Vec::new(),
            errors: vec![format!("{error:#}")],
        }
    }
}

pub fn emit_json(envelope: &Envelope) -> Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, envelope)?;
    writeln!(handle)?;
    Ok(())
}
