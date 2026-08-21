use anyhow::{bail, Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("atomic write target has no parent")?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".skillctl-lock-")
        .tempfile_in(parent)?;
    set_lock_permissions(temporary.as_file(), path)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    if std::env::var_os("SKILLCTL_TEST_FAIL_LOCK_WRITE").is_some() {
        bail!("injected lock write failure");
    }
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("could not atomically replace {}", path.display()))?;
    Ok(())
}

pub fn replace_destination_and_lock(
    staged: &Path,
    destination: &Path,
    lock_path: &Path,
    lock_bytes: &[u8],
) -> Result<()> {
    let parent = destination.parent().context("destination has no parent")?;
    fs::create_dir_all(parent)?;
    let backup = unique_backup(parent, "replace");
    let existed = destination.exists();
    if existed {
        fs::rename(destination, &backup)
            .with_context(|| format!("could not back up destination {}", destination.display()))?;
    }
    if let Err(error) = fs::rename(staged, destination) {
        if existed {
            let _ = fs::rename(&backup, destination);
        }
        return Err(error).context("could not install staged destination");
    }
    if let Err(error) = atomic_write(lock_path, lock_bytes) {
        let _ = fs::remove_dir_all(destination);
        if existed {
            fs::rename(&backup, destination)
                .context("lock write failed and destination rollback also failed")?;
        }
        return Err(error).context("lock write failed; destination was rolled back");
    }
    if existed {
        fs::remove_dir_all(&backup).context("could not remove destination backup")?;
    }
    Ok(())
}

pub fn remove_destination_and_lock(
    destination: Option<&Path>,
    lock_path: &Path,
    lock_bytes: &[u8],
) -> Result<()> {
    let backup = if let Some(destination) = destination {
        let parent = destination.parent().context("destination has no parent")?;
        let backup = unique_backup(parent, "remove");
        fs::rename(destination, &backup).with_context(|| {
            format!(
                "could not stage destination removal {}",
                destination.display()
            )
        })?;
        Some((destination.to_path_buf(), backup))
    } else {
        None
    };
    if let Err(error) = atomic_write(lock_path, lock_bytes) {
        if let Some((destination, backup)) = &backup {
            fs::rename(backup, destination)
                .context("lock write failed and removal rollback also failed")?;
        }
        return Err(error).context("lock write failed; removal was rolled back");
    }
    if let Some((_, backup)) = backup {
        fs::remove_dir_all(backup).context("could not remove staged destination")?;
    }
    Ok(())
}

fn unique_backup(parent: &Path, operation: &str) -> PathBuf {
    let token = format!(
        ".skillctl-{operation}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    parent.join(token)
}

fn set_lock_permissions(file: &fs::File, target: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(target)
        .map(|metadata| metadata.permissions().mode() & 0o777)
        .unwrap_or(0o644);
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    Ok(())
}
