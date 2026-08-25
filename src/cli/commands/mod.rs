mod add;
mod diff;
mod init;
mod inspect;
mod reconcile;
mod remove;
mod support;

use super::args::{Cli, Command};
use crate::lockfile::SourceSelector;
use crate::output::Envelope;
use crate::scope::Scope;
use anyhow::{bail, Result};

pub(super) fn execute(cli: &Cli, scope: &Scope) -> Result<(Envelope, Vec<String>)> {
    match &cli.command {
        Command::Init => init::run(scope, cli.dry_run),
        Command::Add {
            repository,
            path,
            file,
            name,
            reference,
        } => {
            let selector = match (path, file) {
                (Some(path), None) => SourceSelector::Directory(path.clone()),
                (None, Some(file)) => SourceSelector::File(file.clone()),
                _ => bail!("add requires exactly one of --path or --file"),
            };
            add::run(
                scope,
                repository,
                &selector,
                name.as_deref(),
                reference.as_deref(),
                cli.dry_run,
            )
        }
        Command::Sync { names } => reconcile::sync(scope, names, cli.force, cli.dry_run),
        Command::Check { names } => inspect::check(scope, names),
        Command::Status { offline } => inspect::status(scope, *offline),
        Command::Diff { names } => diff::run(scope, names),
        Command::Update { names } => reconcile::update(scope, names, cli.force, cli.dry_run),
        Command::Remove { name } => remove::run(scope, name, cli.dry_run),
        Command::List => inspect::list(scope),
    }
}
