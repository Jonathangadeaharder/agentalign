use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use tempfile::TempDir;

/// Point the binary's home resolution at `sandbox`.
///
/// `dirs::home_dir()` ignores `HOME` on Windows, so setting only `HOME` let these
/// tests run `migrate`/`sync`/`restore` against the developer's real home.
fn with_home<'a>(cmd: &'a mut Command, sandbox: &Path) -> &'a mut Command {
    cmd.env("AGENTALIGN_HOME", sandbox).env("HOME", sandbox)
}

#[test]
fn test_agentalign_restore_list_empty() {
    let sandbox = TempDir::new().unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    let assert = with_home(&mut cmd, sandbox.path())
        .arg("restore")
        .arg("--list")
        .assert();

    assert
        .success()
        .stdout(predicate::str::contains("No transactions found"));
}

#[test]
fn test_agentalign_migrate_dry_run() {
    let sandbox = TempDir::new().unwrap();

    // No agent configs exist yet — dry run should report none found
    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    let assert = with_home(&mut cmd, sandbox.path())
        .arg("migrate")
        .arg("--dry-run")
        .assert();

    assert
        .success()
        .stdout(predicate::str::contains("No existing agent configs found"));
}

#[test]
fn test_agentalign_sync_no_canonical() {
    let sandbox = TempDir::new().unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    let assert = with_home(&mut cmd, sandbox.path())
        .arg("sync")
        .assert();

    // Sync without canonical config should fail with a helpful error
    assert
        .failure()
        .stderr(predicate::str::contains("No canonical config"));
}

#[test]
fn test_agentalign_migrate_creates_agents_dir() {
    let sandbox = TempDir::new().unwrap();
    let agents_dir = sandbox.path().join(".agents");

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    let assert = with_home(&mut cmd, sandbox.path())
        .arg("migrate")
        .arg("--dry-run")
        .assert();

    assert.success();
    // Dry run should NOT create the directory
    assert!(!agents_dir.exists());
}

#[test]
fn test_agentalign_help_output() {
    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    let assert = cmd.arg("--help").assert();

    assert
        .success()
        .stdout(predicate::str::contains("Agent Configuration Unification Engine"));
}
