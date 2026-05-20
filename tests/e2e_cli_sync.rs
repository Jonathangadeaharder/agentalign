use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_agentalign_restore_list_empty() {
    let sandbox = TempDir::new().unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    let assert = cmd
        .arg("restore")
        .arg("--list")
        .env("HOME", sandbox.path())
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
    let assert = cmd
        .arg("migrate")
        .arg("--dry-run")
        .env("HOME", sandbox.path())
        .assert();

    assert
        .success()
        .stdout(predicate::str::contains("No existing agent configs found"));
}

#[test]
fn test_agentalign_sync_no_canonical() {
    let sandbox = TempDir::new().unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    let assert = cmd
        .arg("sync")
        .env("HOME", sandbox.path())
        .assert();

    assert
        .success()
        .stderr(predicate::str::contains("No canonical config found"));
}

#[test]
fn test_agentalign_migrate_creates_agents_dir() {
    let sandbox = TempDir::new().unwrap();
    let agents_dir = sandbox.path().join(".agents");

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    let assert = cmd
        .arg("migrate")
        .arg("--dry-run")
        .env("HOME", sandbox.path())
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
