use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "skillctl", version, about)]
pub struct Cli {
    /// Use the machine-global ~/.agents scope exclusively
    #[arg(short = 'g', long, global = true)]
    pub(super) global: bool,

    /// Emit one stable JSON document on stdout
    #[arg(long, global = true)]
    pub(super) json: bool,

    /// Preview a mutating command without writing files
    #[arg(long, global = true)]
    pub(super) dry_run: bool,

    /// Replace locally changed managed content (sync/update only)
    #[arg(long, global = true)]
    pub(super) force: bool,

    #[command(subcommand)]
    pub(super) command: Command,
}

#[derive(Debug, Subcommand)]
pub(super) enum Command {
    /// Initialize an empty version-2 lock in the current or global scope
    Init,
    /// Add and vendor a skill from a Git repository
    Add {
        repository: String,
        /// Repository-relative skill directory; use . for the repository root
        #[arg(long, conflicts_with = "file", required_unless_present = "file")]
        path: Option<String>,
        /// Repository-relative SKILL.md to install without surrounding files
        #[arg(
            long,
            conflicts_with = "path",
            required_unless_present = "path",
            requires = "name"
        )]
        file: Option<String>,
        /// Lock entry and destination folder name; required with --file
        #[arg(long)]
        name: Option<String>,
        /// Branch, tag, or revision followed by update
        #[arg(long = "ref")]
        reference: Option<String>,
    },
    /// Reproduce exact locked commits without advancing refs; omit NAME for all entries
    Sync {
        #[arg(value_name = "NAME")]
        names: Vec<String>,
    },
    /// Check installed integrity without network access; omit NAME for all entries
    Check {
        #[arg(value_name = "NAME")]
        names: Vec<String>,
    },
    /// Show local integrity and upstream lifecycle state
    Status {
        /// Skip upstream access and report local state only
        #[arg(long)]
        offline: bool,
    },
    /// Compare installed content with the pinned source snapshot; omit NAME for all entries
    Diff {
        #[arg(value_name = "NAME")]
        names: Vec<String>,
    },
    /// Advance vendored entries along their configured source refs; omit NAME for all entries
    Update {
        #[arg(value_name = "NAME")]
        names: Vec<String>,
    },
    /// Remove a lock entry and, when safely managed, its vendored tree
    Remove { name: String },
    /// List lock entries and pins
    List,
}
