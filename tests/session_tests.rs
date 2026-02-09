//! Unit tests for session registration file handling.

use captain_hook::session::registration;
use captain_hook::session::RegistrationEntry;
use chrono::Utc;
use tempfile::TempDir;

fn make_entry(role: &str) -> RegistrationEntry {
    RegistrationEntry {
        role: role.into(),
        task: None,
        prompt_hash: None,
        prompt_path: None,
        registered_at: Utc::now(),
        registered_by: None,
    }
}

fn make_entry_with_task(role: &str, task: &str) -> RegistrationEntry {
    RegistrationEntry {
        role: role.into(),
        task: Some(task.into()),
        prompt_hash: None,
        prompt_path: None,
        registered_at: Utc::now(),
        registered_by: None,
    }
}

// ---------------------------------------------------------------------------
// Registration file: read/write
// ---------------------------------------------------------------------------

#[test]
fn read_nonexistent_file_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("sessions.json");
    let result = registration::read_registration_file(&path).unwrap();
    assert!(result.is_empty());
}

#[test]
fn write_and_read_single_entry() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("sessions.json");

    let entry = make_entry("coder");
    registration::write_registration_entry(&path, "session-1", &entry).unwrap();

    let entries = registration::read_registration_file(&path).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries.contains_key("session-1"));
    assert_eq!(entries["session-1"].role, "coder");
}

#[test]
fn write_multiple_entries() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("sessions.json");

    registration::write_registration_entry(&path, "s1", &make_entry("coder")).unwrap();
    registration::write_registration_entry(&path, "s2", &make_entry("tester")).unwrap();
    registration::write_registration_entry(&path, "s3", &make_entry("maintainer")).unwrap();

    let entries = registration::read_registration_file(&path).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries["s1"].role, "coder");
    assert_eq!(entries["s2"].role, "tester");
    assert_eq!(entries["s3"].role, "maintainer");
}

#[test]
fn overwrite_existing_session() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("sessions.json");

    registration::write_registration_entry(&path, "s1", &make_entry("coder")).unwrap();
    registration::write_registration_entry(&path, "s1", &make_entry("tester")).unwrap();

    let entries = registration::read_registration_file(&path).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries["s1"].role, "tester"); // Updated
}

#[test]
fn write_entry_with_task() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("sessions.json");

    let entry = make_entry_with_task("coder", "implement feature X");
    registration::write_registration_entry(&path, "s1", &entry).unwrap();

    let entries = registration::read_registration_file(&path).unwrap();
    assert_eq!(entries["s1"].task.as_deref(), Some("implement feature X"));
}

// ---------------------------------------------------------------------------
// Registration file: remove
// ---------------------------------------------------------------------------

#[test]
fn remove_existing_entry() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("sessions.json");

    registration::write_registration_entry(&path, "s1", &make_entry("coder")).unwrap();
    registration::write_registration_entry(&path, "s2", &make_entry("tester")).unwrap();

    registration::remove_registration_entry(&path, "s1").unwrap();

    let entries = registration::read_registration_file(&path).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(!entries.contains_key("s1"));
    assert!(entries.contains_key("s2"));
}

#[test]
fn remove_nonexistent_entry_is_noop() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("sessions.json");

    registration::write_registration_entry(&path, "s1", &make_entry("coder")).unwrap();
    registration::remove_registration_entry(&path, "nonexistent").unwrap();

    let entries = registration::read_registration_file(&path).unwrap();
    assert_eq!(entries.len(), 1);
}

#[test]
fn remove_from_nonexistent_file_is_noop() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("sessions.json");
    // File doesn't exist yet -- should not error
    registration::remove_registration_entry(&path, "s1").unwrap();
}

// ---------------------------------------------------------------------------
// Atomic write (tmp file + rename)
// ---------------------------------------------------------------------------

#[test]
fn write_creates_parent_dirs() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("nested").join("dir").join("sessions.json");

    registration::write_registration_entry(&path, "s1", &make_entry("coder")).unwrap();

    let entries = registration::read_registration_file(&path).unwrap();
    assert_eq!(entries.len(), 1);
}

// ---------------------------------------------------------------------------
// RegistrationEntry fields
// ---------------------------------------------------------------------------

#[test]
fn entry_serialization_roundtrip() {
    let entry = RegistrationEntry {
        role: "coder".into(),
        task: Some("build feature".into()),
        prompt_hash: Some("abc123".into()),
        prompt_path: Some("/tmp/prompt.md".into()),
        registered_at: Utc::now(),
        registered_by: Some("user@example.com".into()),
    };

    let json = serde_json::to_string(&entry).unwrap();
    let deserialized: RegistrationEntry = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.role, "coder");
    assert_eq!(deserialized.task.as_deref(), Some("build feature"));
    assert_eq!(deserialized.prompt_hash.as_deref(), Some("abc123"));
    assert_eq!(deserialized.prompt_path.as_deref(), Some("/tmp/prompt.md"));
    assert_eq!(
        deserialized.registered_by.as_deref(),
        Some("user@example.com")
    );
}

// ---------------------------------------------------------------------------
// SessionManager: basic operations with tempfiles
// ---------------------------------------------------------------------------

#[test]
fn session_manager_register_and_check() {
    use captain_hook::session::SessionManager;

    // Use a unique suffix to avoid conflicts with other tests
    let suffix = format!("test-{}", std::process::id());
    let mgr = SessionManager::new(Some(&suffix));

    let session_id = format!(
        "test-session-{}",
        Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );

    // Should not be registered initially (env var might interfere, but without
    // CAPTAIN_HOOK_ROLE set this should be false if the file doesn't exist)
    // Clean up from any previous runs
    let _ = mgr.disable(&session_id);
    let _ = mgr.enable(&session_id);

    // Register the session
    mgr.register(&session_id, "coder", None, None).unwrap();
    assert!(mgr.is_registered(&session_id));

    // Clean up
    let reg_path = std::path::PathBuf::from(format!("/tmp/captain-hook-{suffix}-sessions.json"));
    let exc_path = std::path::PathBuf::from(format!("/tmp/captain-hook-{suffix}-exclusions.json"));
    let _ = std::fs::remove_file(&reg_path);
    let _ = std::fs::remove_file(&exc_path);
}

#[test]
fn session_manager_disable_and_enable() {
    use captain_hook::session::SessionManager;

    let suffix = format!("test-dis-{}", std::process::id());
    let mgr = SessionManager::new(Some(&suffix));

    let session_id = format!("test-dis-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));

    mgr.register(&session_id, "coder", None, None).unwrap();
    assert!(!mgr.is_disabled(&session_id));

    mgr.disable(&session_id).unwrap();
    assert!(mgr.is_disabled(&session_id));

    mgr.enable(&session_id).unwrap();
    assert!(!mgr.is_disabled(&session_id));

    // Clean up
    let reg_path = std::path::PathBuf::from(format!("/tmp/captain-hook-{suffix}-sessions.json"));
    let exc_path = std::path::PathBuf::from(format!("/tmp/captain-hook-{suffix}-exclusions.json"));
    let _ = std::fs::remove_file(&reg_path);
    let _ = std::fs::remove_file(&exc_path);
}

// ---------------------------------------------------------------------------
// Scope level parsing
// ---------------------------------------------------------------------------

#[test]
fn scope_level_from_str() {
    use captain_hook::decision::ScopeLevel;
    use std::str::FromStr;

    assert_eq!(ScopeLevel::from_str("org").unwrap(), ScopeLevel::Org);
    assert_eq!(
        ScopeLevel::from_str("project").unwrap(),
        ScopeLevel::Project
    );
    assert_eq!(ScopeLevel::from_str("user").unwrap(), ScopeLevel::User);
    assert_eq!(ScopeLevel::from_str("role").unwrap(), ScopeLevel::Role);
    assert_eq!(ScopeLevel::from_str("ORG").unwrap(), ScopeLevel::Org);
    assert!(ScopeLevel::from_str("invalid").is_err());
}

#[test]
fn scope_level_display() {
    use captain_hook::decision::ScopeLevel;

    assert_eq!(format!("{}", ScopeLevel::Org), "org");
    assert_eq!(format!("{}", ScopeLevel::Project), "project");
    assert_eq!(format!("{}", ScopeLevel::User), "user");
    assert_eq!(format!("{}", ScopeLevel::Role), "role");
}
