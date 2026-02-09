use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::Result;
use crate::session::RegistrationEntry;

/// Read all registration entries from a file.
pub fn read_registration_file(path: &Path) -> Result<HashMap<String, RegistrationEntry>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let contents = fs::read_to_string(path)?;
    if contents.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let entries: HashMap<String, RegistrationEntry> = serde_json::from_str(&contents)?;
    Ok(entries)
}

/// Write a registration entry to the file with file locking and restrictive permissions.
pub fn write_registration_entry(
    path: &Path,
    session_id: &str,
    entry: &RegistrationEntry,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Acquire an advisory lock on a lockfile to prevent concurrent read-modify-write races
    let _lock = FileLock::acquire(path)?;

    // Read existing entries, add new one, write back atomically.
    let mut entries = read_registration_file(path)?;
    entries.insert(session_id.to_string(), entry.clone());

    let json = serde_json::to_string_pretty(&entries)?;
    let tmp_path = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
    }
    set_file_permissions_0600(&tmp_path);
    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Remove a registration entry from the file with file locking.
pub fn remove_registration_entry(path: &Path, session_id: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let _lock = FileLock::acquire(path)?;

    let mut entries = read_registration_file(path)?;
    entries.remove(session_id);

    let json = serde_json::to_string_pretty(&entries)?;
    let tmp_path = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
    }
    set_file_permissions_0600(&tmp_path);
    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Set file permissions to 0600 (owner read/write only).
#[cfg(unix)]
fn set_file_permissions_0600(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(0o600);
    let _ = fs::set_permissions(path, perms);
}

#[cfg(not(unix))]
fn set_file_permissions_0600(_path: &Path) {
    // No-op on non-Unix platforms
}

/// Advisory file lock using flock(2) on a .lock file.
struct FileLock {
    _file: fs::File,
}

impl FileLock {
    fn acquire(path: &Path) -> Result<Self> {
        let lock_path = path.with_extension("lock");
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)?;
        flock_exclusive(&file)?;
        Ok(Self { _file: file })
    }
}

// When FileLock is dropped, the file is closed and the lock is released.

#[cfg(unix)]
fn flock_exclusive(file: &fs::File) -> Result<()> {
    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    // LOCK_EX = 2 (exclusive lock)
    let ret = unsafe { libc::flock(fd, libc::LOCK_EX) };
    if ret != 0 {
        return Err(crate::error::CaptainHookError::Io(
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn flock_exclusive(_file: &fs::File) -> Result<()> {
    // No-op on non-Unix platforms
    Ok(())
}
