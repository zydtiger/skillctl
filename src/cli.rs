use crate::error::CommandFailure;
use crate::install::{
    compare_trees, copy_snapshot, entry_state, marker_for, read_marker, skill_json,
    validate_declared_names, validate_destination_location, validate_scope_layout, write_marker,
};
use crate::lockfile::{validate_identifier, LockFile, Mode, Resolved, SkillEntry, SourceSpec};
use crate::output::{emit_json, Envelope};
use crate::scope::Scope;
use crate::source::{acquire, validate_skill_tree, Snapshot};
use crate::transaction::{atomic_write, remove_destination_and_lock, replace_destination_and_lock};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::collections::BTreeMap;
use std::env;
use std::fs;

#[derive(Debug, Parser)]
#[command(name = "skillctl", version, about)]
pub struct Cli {
    /// Use the machine-global ~/.agents scope exclusively
    #[arg(short = 'g', long, global = true)]
    global: bool,

    /// Emit one stable JSON document on stdout
    #[arg(long, global = true)]
    json: bool,

    /// Preview a mutating command without writing files
    #[arg(long, global = true)]
    dry_run: bool,

    /// Replace locally changed managed content (sync/update only)
    #[arg(long, global = true)]
    force: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize an empty version-1 lock in the current or global scope
    Init,
    /// Add and vendor a skill from a Git repository
    Add {
        repository: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long = "ref")]
        reference: Option<String>,
    },
    /// Reproduce exact locked commits without advancing refs
    Sync { name: Option<String> },
    /// Check installed integrity without network access
    Check { name: Option<String> },
    /// Show the local state of every lock entry without network access
    Status,
    /// Compare installed content with the pinned source snapshot
    Diff { name: Option<String> },
    /// Advance vendored entries along their configured source refs
    Update { name: Option<String> },
    /// Remove a lock entry and, when safely managed, its vendored tree
    Remove { name: String },
    /// List lock entries and pins
    List,
}

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
    match execute(&cli, &scope) {
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

fn execute(cli: &Cli, scope: &Scope) -> Result<(Envelope, Vec<String>)> {
    match &cli.command {
        Command::Init => init(scope, cli.dry_run),
        Command::Add {
            repository,
            path,
            name,
            reference,
        } => add(
            scope,
            repository,
            path,
            name.as_deref(),
            reference.as_deref(),
            cli.dry_run,
        ),
        Command::Sync { name } => sync(scope, name.as_deref(), cli.force, cli.dry_run),
        Command::Check { name } => check(scope, name.as_deref()),
        Command::Status => status(scope),
        Command::Diff { name } => diff(scope, name.as_deref()),
        Command::Update { name } => update(scope, name.as_deref(), cli.force, cli.dry_run),
        Command::Remove { name } => remove(scope, name, cli.dry_run),
        Command::List => list(scope),
    }
}

fn init(scope: &Scope, dry_run: bool) -> Result<(Envelope, Vec<String>)> {
    validate_scope_layout(scope)?;
    if scope.lock_file.exists() {
        bail!(
            "refusing to overwrite existing lock {}",
            scope.lock_file.display()
        );
    }
    let lock = LockFile::empty();
    let mut envelope = Envelope::new(scope);
    envelope.changes.push(json!({
        "action": "initialize",
        "path": scope.lock_file,
        "dry_run": dry_run,
    }));
    if !dry_run {
        fs::create_dir_all(&scope.skills_dir)?;
        atomic_write(&scope.lock_file, &lock.yaml()?)?;
    }
    Ok((
        envelope,
        vec![if dry_run {
            format!(
                "would initialize {} scope at {}",
                scope.label(),
                scope.lock_file.display()
            )
        } else {
            format!(
                "initialized {} scope at {}",
                scope.label(),
                scope.lock_file.display()
            )
        }],
    ))
}

fn add(
    scope: &Scope,
    repository: &str,
    source_path: &str,
    requested_name: Option<&str>,
    reference: Option<&str>,
    dry_run: bool,
) -> Result<(Envelope, Vec<String>)> {
    let mut lock = load_lock(scope)?;
    validate_declared_names(scope, &lock)?;
    let repository = normalize_repository(repository)?;
    let snapshot = acquire(&repository, source_path, reference)?;
    let name = requested_name.unwrap_or(&snapshot.metadata.name);
    validate_identifier(name).context("invalid --name")?;
    if lock.skills.contains_key(name) {
        bail!("lock entry `{name}` already exists");
    }
    if lock.skills.values().any(|entry| entry.destination == name) {
        bail!("destination `{name}` already appears in the lock");
    }
    let destination = scope.skills_dir.join(name);
    validate_destination_location(scope, &entry_for_destination(name))?;
    if destination.exists() {
        bail!(
            "destination {} already exists and is unmanaged",
            destination.display()
        );
    }
    for (existing_name, existing) in &lock.skills {
        let existing_path = scope.skills_dir.join(&existing.destination);
        if existing_path.exists() {
            let metadata = validate_skill_tree(&existing_path)?;
            if metadata.name == snapshot.metadata.name {
                bail!(
                    "declared skill name `{}` collides with lock entry `{existing_name}`",
                    snapshot.metadata.name
                );
            }
        }
    }
    let entry = SkillEntry {
        mode: Mode::Vendored,
        source: Some(SourceSpec {
            repository: repository.clone(),
            path: source_path.to_owned(),
            reference: snapshot.reference.clone(),
        }),
        resolved: Some(Resolved {
            commit: snapshot.commit.clone(),
            digest: snapshot.digest.clone(),
        }),
        destination: name.to_owned(),
    };
    lock.skills.insert(name.to_owned(), entry.clone());
    lock.validate()?;
    let mut envelope = Envelope::new(scope);
    envelope.skills.push(skill_json(name, &entry, None));
    envelope.changes.push(json!({
        "action": "add",
        "name": name,
        "destination": entry.destination,
        "commit": snapshot.commit,
        "digest": snapshot.digest,
        "dry_run": dry_run,
    }));
    if !dry_run {
        install_prepared(scope, name, &entry, &snapshot, &lock, false)?;
    }
    Ok((
        envelope,
        vec![format!(
            "{} `{name}` at {} ({})",
            if dry_run { "would add" } else { "added" },
            snapshot.commit,
            snapshot.digest
        )],
    ))
}

fn normalize_repository(repository: &str) -> Result<String> {
    let path = std::path::Path::new(repository);
    if path.exists() {
        Ok(fs::canonicalize(path)?
            .to_str()
            .context("local repository path is not valid UTF-8")?
            .to_owned())
    } else {
        Ok(repository.to_owned())
    }
}

fn sync(
    scope: &Scope,
    selected: Option<&str>,
    force: bool,
    dry_run: bool,
) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let names = select_names(&lock, selected)?;
    let mut prepared = Vec::new();
    let mut skipped = Vec::new();
    for name in names {
        let entry = lock.skills.get(&name).expect("selected entry");
        if entry.mode == Mode::Local {
            if selected.is_some() {
                bail!("cannot sync local entry `{name}`; local files are project-owned");
            }
            skipped.push(name);
            continue;
        }
        let source = entry.source.as_ref().expect("validated lock");
        let resolved = entry.resolved.as_ref().expect("validated lock");
        let snapshot = acquire(&source.repository, &source.path, Some(&resolved.commit))?;
        if snapshot.commit != resolved.commit || snapshot.digest != resolved.digest {
            bail!(
                "pinned snapshot for `{name}` does not match lock: got {} {}, expected {} {}",
                snapshot.commit,
                snapshot.digest,
                resolved.commit,
                resolved.digest
            );
        }
        ensure_replace_allowed(scope, &name, entry, &snapshot, force)?;
        prepared.push((name, snapshot));
    }
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    for (name, snapshot) in &prepared {
        let entry = lock.skills.get(name).expect("prepared entry");
        envelope.changes.push(json!({
            "action": "sync",
            "name": name,
            "commit": snapshot.commit,
            "dry_run": dry_run,
        }));
        if !dry_run {
            install_prepared(scope, name, entry, snapshot, &lock, true)?;
        }
        lines.push(format!(
            "{} `{name}` to {}",
            if dry_run { "would sync" } else { "synced" },
            snapshot.commit
        ));
    }
    for name in skipped {
        envelope
            .changes
            .push(json!({"action":"skip-local", "name":name}));
        lines.push(format!("left local entry `{name}` untouched"));
    }
    Ok((envelope, lines))
}

fn check(scope: &Scope, selected: Option<&str>) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let names = select_names(&lock, selected)?;
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    let mut failures = Vec::new();
    let mut declared: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (entry_name, entry) in &lock.skills {
        let destination = scope.skills_dir.join(&entry.destination);
        if let Ok(metadata) = validate_skill_tree(&destination) {
            declared
                .entry(metadata.name)
                .or_default()
                .push(entry_name.clone());
        }
    }
    for name in names {
        let entry = lock.skills.get(&name).expect("selected entry");
        let state = entry_state(scope, &name, entry);
        if !matches!(state.state.as_str(), "clean" | "local") {
            failures.push(format!(
                "`{name}` is {}: {}",
                state.state,
                state.details.join("; ")
            ));
        }
        lines.push(format!("{name}: {}", state.state));
        envelope.skills.push(skill_json(&name, entry, Some(&state)));
    }
    for (declared_name, entries) in declared {
        if entries.len() > 1
            && selected
                .map(|selected| entries.iter().any(|entry| entry == selected))
                .unwrap_or(true)
        {
            failures.push(format!(
                "declared skill name `{declared_name}` is duplicated by {}",
                entries.join(", ")
            ));
        }
    }
    if !failures.is_empty() {
        envelope.ok = false;
        envelope.errors = failures.clone();
        return Err(CommandFailure {
            envelope,
            message: format!("integrity check failed: {}", failures.join("; ")),
        }
        .into());
    }
    Ok((envelope, lines))
}

fn status(scope: &Scope) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    let mut states: BTreeMap<String, crate::install::EntryState> = lock
        .skills
        .iter()
        .map(|(name, entry)| (name.clone(), entry_state(scope, name, entry)))
        .collect();
    let mut declared: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, state) in &states {
        if let Some(metadata) = &state.metadata {
            declared
                .entry(metadata.name.clone())
                .or_default()
                .push(name.clone());
        }
    }
    for (declared_name, entries) in declared {
        if entries.len() > 1 {
            for name in entries {
                let state = states.get_mut(&name).expect("known status entry");
                state.state = "invalid".to_owned();
                state.details.push(format!(
                    "declared skill name `{declared_name}` is used by multiple destinations"
                ));
            }
        }
    }
    for (name, entry) in &lock.skills {
        let state = states.get(name).expect("known status entry");
        let mut record = skill_json(name, entry, Some(state));
        record["update_status"] = json!("unknown");
        envelope.skills.push(record);
        lines.push(format!("{name}: {} (update status unknown)", state.state));
    }
    Ok((envelope, lines))
}

fn diff(scope: &Scope, selected: Option<&str>) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let names = select_names(&lock, selected)?;
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    for name in names {
        let entry = lock.skills.get(&name).expect("selected entry");
        if entry.mode == Mode::Local {
            if selected.is_some() {
                bail!("cannot diff local entry `{name}` against a pinned source");
            }
            continue;
        }
        validate_destination_location(scope, entry)?;
        let installed = scope.skills_dir.join(&entry.destination);
        if !installed.exists() {
            bail!("cannot diff missing destination for `{name}`");
        }
        let source = entry.source.as_ref().expect("validated lock");
        let resolved = entry.resolved.as_ref().expect("validated lock");
        let snapshot = acquire(&source.repository, &source.path, Some(&resolved.commit))?;
        if snapshot.digest != resolved.digest {
            bail!("pinned source digest for `{name}` no longer matches the lock");
        }
        let changes = compare_trees(&installed, &snapshot.root)?;
        if changes.is_empty() {
            lines.push(format!("{name}: no differences"));
        }
        for change in changes {
            lines.push(format!("{name}: {} {}", change.change, change.path));
            envelope.changes.push(json!({
                "name": name,
                "path": change.path,
                "change": change.change,
            }));
        }
    }
    Ok((envelope, lines))
}

fn update(
    scope: &Scope,
    selected: Option<&str>,
    force: bool,
    dry_run: bool,
) -> Result<(Envelope, Vec<String>)> {
    let mut lock = load_lock(scope)?;
    let names = select_names(&lock, selected)?;
    let original = lock.clone();
    let mut prepared = Vec::new();
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    for name in names {
        let old = original.skills.get(&name).expect("selected entry");
        if old.mode == Mode::Local {
            if selected.is_some() {
                bail!("cannot update local entry `{name}`; local files are project-owned");
            }
            lines.push(format!("left local entry `{name}` untouched"));
            continue;
        }
        let source = old.source.as_ref().expect("validated lock");
        let snapshot = acquire(&source.repository, &source.path, Some(&source.reference))?;
        let resolved = old.resolved.as_ref().expect("validated lock");
        if snapshot.commit == resolved.commit && snapshot.digest == resolved.digest {
            lines.push(format!("{name}: already up to date at {}", resolved.commit));
            envelope.changes.push(json!({
                "action": "no-op",
                "name": name,
                "commit": resolved.commit,
            }));
            continue;
        }
        ensure_replace_allowed(scope, &name, old, &snapshot, force)?;
        prepared.push((name, snapshot));
    }
    for (name, snapshot) in &prepared {
        let updated = lock.skills.get_mut(name).expect("prepared entry");
        updated.resolved = Some(Resolved {
            commit: snapshot.commit.clone(),
            digest: snapshot.digest.clone(),
        });
        lock.validate()?;
        let entry = lock.skills.get(name).expect("prepared entry");
        envelope.changes.push(json!({
            "action": "update",
            "name": name,
            "commit": snapshot.commit,
            "digest": snapshot.digest,
            "dry_run": dry_run,
        }));
        if !dry_run {
            install_prepared(scope, name, entry, snapshot, &lock, true)?;
        }
        lines.push(format!(
            "{} `{name}` to {}",
            if dry_run { "would update" } else { "updated" },
            snapshot.commit
        ));
    }
    Ok((envelope, lines))
}

fn remove(scope: &Scope, name: &str, dry_run: bool) -> Result<(Envelope, Vec<String>)> {
    let mut lock = load_lock(scope)?;
    let entry = lock
        .skills
        .get(name)
        .cloned()
        .with_context(|| format!("lock entry `{name}` does not exist"))?;
    let destination = scope.skills_dir.join(&entry.destination);
    let remove_destination = if entry.mode == Mode::Vendored && destination.exists() {
        validate_destination_location(scope, &entry)?;
        crate::digest::inspect_tree(&destination)
            .with_context(|| format!("refusing to delete unsafe destination for `{name}`"))?;
        let marker = read_marker(&destination)
            .with_context(|| format!("refusing to delete unmanaged destination for `{name}`"))?;
        if marker != marker_for(scope, name, &entry)? {
            bail!("refusing to delete destination for `{name}` because its marker does not match");
        }
        Some(destination.as_path())
    } else {
        None
    };
    lock.skills.remove(name);
    let bytes = lock.yaml()?;
    let mut envelope = Envelope::new(scope);
    envelope.changes.push(json!({
        "action": "remove",
        "name": name,
        "destination_removed": remove_destination.is_some(),
        "local_files_remain": entry.mode == Mode::Local,
        "dry_run": dry_run,
    }));
    if !dry_run {
        remove_destination_and_lock(remove_destination, &scope.lock_file, &bytes)?;
    }
    let detail = if entry.mode == Mode::Local {
        "local files remain"
    } else if remove_destination.is_some() {
        "managed files removed"
    } else {
        "destination was already missing"
    };
    Ok((
        envelope,
        vec![format!(
            "{} `{name}` from lock ({detail})",
            if dry_run { "would remove" } else { "removed" }
        )],
    ))
}

fn list(scope: &Scope) -> Result<(Envelope, Vec<String>)> {
    let lock = load_lock(scope)?;
    let mut envelope = Envelope::new(scope);
    let mut lines = Vec::new();
    for (name, entry) in &lock.skills {
        envelope.skills.push(skill_json(name, entry, None));
        let source = entry
            .source
            .as_ref()
            .map(|source| format!("{}:{}@{}", source.repository, source.path, source.reference))
            .unwrap_or_else(|| "-".to_owned());
        let commit = entry
            .resolved
            .as_ref()
            .map(|resolved| resolved.commit.as_str())
            .unwrap_or("-");
        lines.push(format!(
            "{name}\t{}\t{}\t{source}\t{commit}",
            match entry.mode {
                Mode::Vendored => "vendored",
                Mode::Local => "local",
            },
            entry.destination
        ));
    }
    Ok((envelope, lines))
}

fn load_lock(scope: &Scope) -> Result<LockFile> {
    validate_scope_layout(scope)?;
    LockFile::load(&scope.lock_file)
}

fn select_names(lock: &LockFile, selected: Option<&str>) -> Result<Vec<String>> {
    if let Some(name) = selected {
        if !lock.skills.contains_key(name) {
            bail!("lock entry `{name}` does not exist");
        }
        Ok(vec![name.to_owned()])
    } else {
        Ok(lock.skills.keys().cloned().collect())
    }
}

fn ensure_replace_allowed(
    scope: &Scope,
    name: &str,
    entry: &SkillEntry,
    expected: &Snapshot,
    force: bool,
) -> Result<()> {
    validate_destination_location(scope, entry)?;
    let destination = scope.skills_dir.join(&entry.destination);
    if !destination.exists() {
        return Ok(());
    }
    let state = entry_state(scope, name, entry);
    if state.state == "clean" {
        return Ok(());
    }
    let changes = compare_trees(&destination, &expected.root)
        .map(|changes| {
            changes
                .into_iter()
                .map(|change| format!("{} {}", change.change, change.path))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|_| state.details.join("; "));
    if !force {
        bail!("refusing to replace changed destination for `{name}` without --force: {changes}");
    }
    Ok(())
}

fn install_prepared(
    scope: &Scope,
    name: &str,
    entry: &SkillEntry,
    snapshot: &Snapshot,
    lock: &LockFile,
    _replacing: bool,
) -> Result<()> {
    validate_destination_location(scope, entry)?;
    let staged = copy_snapshot(&snapshot.root, &scope.skills_dir)?;
    let result = (|| {
        write_marker(&staged, &marker_for(scope, name, entry)?)?;
        let actual = crate::digest::digest_tree(&staged)?;
        let expected = &entry.resolved.as_ref().expect("vendored entry").digest;
        if &actual != expected {
            bail!("staged digest for `{name}` changed unexpectedly");
        }
        replace_destination_and_lock(
            &staged,
            &scope.skills_dir.join(&entry.destination),
            &scope.lock_file,
            &lock.yaml()?,
        )
    })();
    if staged.exists() {
        let _ = fs::remove_dir_all(&staged);
    }
    result
}

fn entry_for_destination(destination: &str) -> SkillEntry {
    SkillEntry {
        mode: Mode::Local,
        source: None,
        resolved: None,
        destination: destination.to_owned(),
    }
}
