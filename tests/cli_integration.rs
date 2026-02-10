//! CLI integration tests using assert_cmd to exercise the actual binary.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn hookwise() -> Command {
    Command::cargo_bin("hookwise").unwrap()
}

// ---------------------------------------------------------------------------
// Init subcommand
// ---------------------------------------------------------------------------

#[test]
fn cli_init_creates_directory_structure() {
    let tmp = TempDir::new().unwrap();

    hookwise()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("initialized .hookwise/"));

    // Verify the expected files were created
    assert!(tmp.path().join(".hookwise").exists());
    assert!(tmp.path().join(".hookwise/policy.yml").exists());
    assert!(tmp.path().join(".hookwise/roles.yml").exists());
    assert!(tmp.path().join(".hookwise/rules").is_dir());
    assert!(tmp.path().join(".hookwise/.index").is_dir());
    assert!(tmp.path().join(".hookwise/.user").is_dir());
    assert!(tmp.path().join(".hookwise/.gitignore").exists());
    assert!(tmp.path().join(".hookwise/rules/allow.jsonl").exists());
    assert!(tmp.path().join(".hookwise/rules/deny.jsonl").exists());
    assert!(tmp.path().join(".hookwise/rules/ask.jsonl").exists());
}

#[test]
fn cli_init_idempotent() {
    let tmp = TempDir::new().unwrap();

    // First init
    hookwise()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    // Second init should succeed (already exists message)
    hookwise()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("already exists"));
}

// ---------------------------------------------------------------------------
// Register subcommand
// ---------------------------------------------------------------------------

#[test]
fn cli_register_unknown_role_fails() {
    let tmp = TempDir::new().unwrap();

    // Init first so roles.yml exists
    hookwise()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    // Register with an unknown role
    hookwise()
        .args([
            "register",
            "--session-id",
            "test-123",
            "--role",
            "nonexistent",
        ])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown role"));
}

#[test]
fn cli_register_known_role_succeeds() {
    let tmp = TempDir::new().unwrap();

    // Init to create roles.yml with coder/tester/maintainer
    hookwise()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    // Register as coder
    hookwise()
        .args(["register", "--session-id", "test-456", "--role", "coder"])
        .current_dir(tmp.path())
        .env_remove("CLAUDE_TEAM_ID")
        .assert()
        .success()
        .stderr(predicate::str::contains("registered as 'coder'"));
}

// ---------------------------------------------------------------------------
// Disable / Enable
// ---------------------------------------------------------------------------

#[test]
fn cli_disable_and_enable() {
    let tmp = TempDir::new().unwrap();

    hookwise()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    // Disable
    hookwise()
        .args(["disable", "--session-id", "test-789"])
        .current_dir(tmp.path())
        .env_remove("CLAUDE_TEAM_ID")
        .assert()
        .success()
        .stderr(predicate::str::contains("disabled"));

    // Enable
    hookwise()
        .args(["enable", "--session-id", "test-789"])
        .current_dir(tmp.path())
        .env_remove("CLAUDE_TEAM_ID")
        .assert()
        .success()
        .stderr(predicate::str::contains("re-enabled"));
}

// ---------------------------------------------------------------------------
// Config subcommand
// ---------------------------------------------------------------------------

#[test]
fn cli_config_shows_project_config() {
    let tmp = TempDir::new().unwrap();

    hookwise()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    hookwise()
        .arg("config")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Project config:"));
}

#[test]
fn cli_config_without_init_shows_not_initialized() {
    let tmp = TempDir::new().unwrap();

    hookwise()
        .arg("config")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("not initialized"));
}

// ---------------------------------------------------------------------------
// Sync subcommand (placeholder)
// ---------------------------------------------------------------------------

#[test]
fn cli_sync_reports_not_implemented() {
    hookwise()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("not yet implemented"));
}

// ---------------------------------------------------------------------------
// Stats subcommand
// ---------------------------------------------------------------------------

#[test]
fn cli_stats_runs_without_error() {
    let tmp = TempDir::new().unwrap();

    hookwise()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    hookwise()
        .arg("stats")
        .current_dir(tmp.path())
        .env_remove("CLAUDE_TEAM_ID")
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Check subcommand (hook mode via stdin)
// ---------------------------------------------------------------------------

#[test]
fn cli_check_with_no_stdin_fails() {
    // check reads from stdin; empty/no stdin should fail
    hookwise()
        .arg("check")
        .write_stdin("")
        .assert()
        .failure();
}

#[test]
fn cli_check_with_invalid_json_fails() {
    hookwise()
        .arg("check")
        .write_stdin("not json")
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// Queue subcommand
// ---------------------------------------------------------------------------

#[test]
fn cli_queue_runs_without_error() {
    let tmp = TempDir::new().unwrap();

    hookwise()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    hookwise()
        .arg("queue")
        .current_dir(tmp.path())
        .env_remove("CLAUDE_TEAM_ID")
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Build subcommand
// ---------------------------------------------------------------------------

#[test]
fn cli_build_runs_without_error() {
    let tmp = TempDir::new().unwrap();

    hookwise()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    hookwise()
        .arg("build")
        .current_dir(tmp.path())
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Scan subcommand
// ---------------------------------------------------------------------------

#[test]
fn cli_scan_on_rules_dir() {
    let tmp = TempDir::new().unwrap();

    hookwise()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    // Scan a specific path (the rules directory) instead of --staged which requires git
    let rules_dir = tmp.path().join(".hookwise").join("rules");
    hookwise()
        .args(["scan", &rules_dir.to_string_lossy()])
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn cli_scan_staged_requires_git() {
    let tmp = TempDir::new().unwrap();

    // --staged without a git repo should fail gracefully
    hookwise()
        .args(["scan", "--staged"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a git repo"));
}

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

#[test]
fn cli_help() {
    hookwise()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Intelligent permission gating"));
}

#[test]
fn cli_version() {
    hookwise()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("hookwise"));
}

#[test]
fn cli_no_args_shows_help() {
    hookwise()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}
