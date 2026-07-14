mod args;
mod commands;

pub use args::Cli;

use crate::error::CommandFailure;
use crate::output::{emit_json, Envelope};
use crate::scope::Scope;
use anyhow::{bail, Result};
use args::Command;
use clap::Parser;
use std::env;

pub fn run() -> Result<()> {
    let arguments: Vec<_> = env::args_os().collect();
    let wants_json = arguments.iter().any(|argument| argument == "--json");
    let wants_global = arguments
        .iter()
        .any(|argument| argument == "--global" || argument == "-g");
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            error.print()?;
            return Ok(());
        }
        Err(error) => {
            let failure = anyhow::anyhow!(error.to_string());
            if wants_json {
                emit_json(&Envelope::failure(None, wants_global, &failure))?;
            } else {
                error.print()?;
            }
            return Err(anyhow::anyhow!("invalid command line"));
        }
    };
    if let Err(error) = validate_flags(&cli) {
        if cli.json {
            emit_json(&Envelope::failure(None, cli.global, &error))?;
        }
        return Err(error);
    }
    let cwd = env::current_dir()?;
    let scope_result = if matches!(cli.command, Command::Init) {
        Scope::for_init(cli.global, &cwd)
    } else {
        Scope::discover(cli.global, &cwd)
    };
    let scope = match scope_result {
        Ok(scope) => scope,
        Err(error) => {
            if cli.json {
                emit_json(&Envelope::failure(None, cli.global, &error))?;
            }
            return Err(error);
        }
    };
    match commands::execute(&cli, &scope) {
        Ok((envelope, lines)) => {
            if cli.json {
                emit_json(&envelope)?;
            } else {
                for line in lines {
                    println!("{line}");
                }
            }
            Ok(())
        }
        Err(error) => {
            if cli.json {
                if let Some(failure) = error.downcast_ref::<CommandFailure>() {
                    emit_json(&failure.envelope)?;
                } else {
                    emit_json(&Envelope::failure(Some(&scope), cli.global, &error))?;
                }
            }
            Err(error)
        }
    }
}

fn validate_flags(cli: &Cli) -> Result<()> {
    let mutating = matches!(
        cli.command,
        Command::Init
            | Command::Add { .. }
            | Command::Sync { .. }
            | Command::Update { .. }
            | Command::Remove { .. }
    );
    if cli.dry_run && !mutating {
        bail!("--dry-run is only valid with init, add, sync, update, or remove");
    }
    if cli.force && !matches!(cli.command, Command::Sync { .. } | Command::Update { .. }) {
        bail!("--force is only valid with sync or update");
    }
    Ok(())
}
