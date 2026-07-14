use crate::output::Envelope;
use std::fmt;

#[derive(Debug)]
pub struct CommandFailure {
    pub envelope: Envelope,
    pub message: String,
}

impl fmt::Display for CommandFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CommandFailure {}
