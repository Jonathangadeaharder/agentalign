use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use tempfile::TempDir;
use std::fs;

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

#[test]
fn sync_imports_an_opencode_only_server_without_exporting_it() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    let opencode = home.join(".config/opencode/opencode.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{}}"#).unwrap();
    fs::write(&opencode, r#"{"mcp":{"playwright":{"type":"local","command":["node","browser.js"],"enabled":true}},"permission":{"playwright_*":"deny"}}"#).unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();

    let canonical: serde_json::Value = serde_json::from_str(&fs::read_to_string(canonical).unwrap()).unwrap();
    let opencode: serde_json::Value = serde_json::from_str(&fs::read_to_string(opencode).unwrap()).unwrap();
    let claude: serde_json::Value = serde_json::from_str(&fs::read_to_string(home.join(".claude.json")).unwrap()).unwrap();
    assert_eq!(canonical["agents"]["opencode"]["playwright"]["command"], serde_json::json!(["node", "browser.js"]));
    assert_eq!(opencode["mcp"]["playwright"]["enabled"], true);
    assert_eq!(opencode["permission"]["playwright_*"], "deny");
    assert!(claude["mcpServers"].get("playwright").is_none());
}

#[test]
fn sync_keeps_different_same_name_servers_per_agent() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    let opencode = home.join(".config/opencode/opencode.json");
    let claude = home.join(".claude.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{}}"#).unwrap();
    fs::write(&opencode, r#"{"mcp":{"search":{"type":"local","command":["node","first.js"]}}}"#).unwrap();
    fs::write(&claude, r#"{"mcpServers":{"search":{"command":"node","args":["other.js"]}}}"#).unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();

    let canonical: serde_json::Value = serde_json::from_str(&fs::read_to_string(canonical).unwrap()).unwrap();
    let opencode: serde_json::Value = serde_json::from_str(&fs::read_to_string(opencode).unwrap()).unwrap();
    let claude: serde_json::Value = serde_json::from_str(&fs::read_to_string(claude).unwrap()).unwrap();
    assert_eq!(canonical["agents"]["opencode"]["search"]["command"], serde_json::json!(["node", "first.js"]));
    assert_eq!(canonical["agents"]["claude"]["search"]["command"], serde_json::json!(["node", "other.js"]));
    assert_eq!(opencode["mcp"]["search"]["command"], serde_json::json!(["node", "first.js"]));
    assert_eq!(claude["mcpServers"]["search"]["args"], serde_json::json!(["other.js"]));
}

#[test]
fn sync_dry_run_does_not_write_and_bad_agent_config_blocks_all_writes() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    let opencode = home.join(".config/opencode/opencode.json");
    let claude = home.join(".claude.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{}}"#).unwrap();
    fs::write(&opencode, r#"{"mcp":{"playwright":{"type":"local","command":["node"]}}}"#).unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).args(["sync", "--dry-run"]).assert().success();
    assert_eq!(fs::read_to_string(&canonical).unwrap(), r#"{"mcp":{}}"#);
    assert!(!claude.exists());

    fs::write(&claude, "not-json").unwrap();
    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().failure();
    assert_eq!(fs::read_to_string(&canonical).unwrap(), r#"{"mcp":{}}"#);
    assert_eq!(fs::read_to_string(&opencode).unwrap(), r#"{"mcp":{"playwright":{"type":"local","command":["node"]}}}"#);
}

#[test]
fn removing_a_published_server_from_one_agent_keeps_the_other_variant() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    let opencode = home.join(".config/opencode/opencode.json");
    let claude = home.join(".claude.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{}}"#).unwrap();
    fs::write(&opencode, r#"{"mcp":{"shared":{"type":"local","command":["node","first.js"]}}}"#).unwrap();
    fs::write(&claude, r#"{"mcpServers":{"shared":{"command":"node","args":["other.js"]}}}"#).unwrap();
    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();
    fs::write(&opencode, r#"{"mcp":{}}"#).unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();

    let canonical: serde_json::Value = serde_json::from_str(&fs::read_to_string(canonical).unwrap()).unwrap();
    let opencode: serde_json::Value = serde_json::from_str(&fs::read_to_string(opencode).unwrap()).unwrap();
    let claude: serde_json::Value = serde_json::from_str(&fs::read_to_string(claude).unwrap()).unwrap();
    assert!(canonical["agents"]["opencode"].get("shared").is_none());
    assert!(opencode["mcp"].get("shared").is_none());
    assert_eq!(claude["mcpServers"]["shared"]["args"], serde_json::json!(["other.js"]));
}

#[test]
fn removing_a_published_shared_server_from_one_agent_does_not_restore_it() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    let opencode = home.join(".config/opencode/opencode.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{"shared":{"type":"local","command":["node"]}}}"#).unwrap();
    fs::write(&opencode, r#"{"mcp":{"shared":{"type":"local","command":["node"]}}}"#).unwrap();
    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();
    fs::write(&opencode, r#"{"mcp":{}}"#).unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();

    let canonical: serde_json::Value = serde_json::from_str(&fs::read_to_string(canonical).unwrap()).unwrap();
    let opencode: serde_json::Value = serde_json::from_str(&fs::read_to_string(opencode).unwrap()).unwrap();
    assert!(canonical["excluded"]["opencode"].as_array().unwrap().contains(&serde_json::json!("shared")));
    assert!(opencode["mcp"].get("shared").is_none());
}

#[test]
fn sync_preserves_native_server_fields_across_repeated_runs() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    let opencode = home.join(".config/opencode/opencode.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{}}"#).unwrap();
    fs::write(&opencode, r#"{"mcp":{"playwright":{"type":"local","command":["node","cli.js"],"enabled":true,"timeout":30000}},"permission":{"playwright_*":"deny"}}"#).unwrap();

    for _ in 0..2 {
        let mut cmd = Command::cargo_bin("agentalign").unwrap();
        with_home(&mut cmd, home).arg("sync").assert().success();
    }

    let canonical: serde_json::Value = serde_json::from_str(&fs::read_to_string(canonical).unwrap()).unwrap();
    let opencode: serde_json::Value = serde_json::from_str(&fs::read_to_string(opencode).unwrap()).unwrap();
    assert_eq!(canonical["agents"]["opencode"]["playwright"]["timeout"], 30000);
    assert_eq!(opencode["mcp"]["playwright"]["timeout"], 30000);
    assert_eq!(opencode["permission"]["playwright_*"], "deny");
}

#[test]
fn sync_keeps_native_fields_in_other_agent_configs() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    let gemini = home.join(".gemini/settings.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::create_dir_all(gemini.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{}}"#).unwrap();
    let native = r#"{"mcpServers":{"search":{"command":"node","args":["index.js"],"env":{"TOKEN":"value"},"trust":true}},"theme":"dark"}"#;
    fs::write(&gemini, native).unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();

    let result: serde_json::Value = serde_json::from_str(&fs::read_to_string(&gemini).unwrap()).unwrap();
    assert_eq!(result["mcpServers"]["search"]["env"]["TOKEN"], "value");
    assert_eq!(result["mcpServers"]["search"]["trust"], true);
    assert_eq!(result["theme"], "dark");
}

#[test]
fn sync_preserves_a_shared_server_skipped_for_one_agent() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{"shared":{"type":"local","command":["node"]}}}"#).unwrap();
    fs::write(home.join(".agents/agent_skip.json"), r#"{"OpenCode":["shared"]}"#).unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();

    let opencode: serde_json::Value = serde_json::from_str(&fs::read_to_string(home.join(".config/opencode/opencode.json")).unwrap()).unwrap();
    let claude: serde_json::Value = serde_json::from_str(&fs::read_to_string(home.join(".claude.json")).unwrap()).unwrap();
    assert!(opencode["mcp"].get("shared").is_none());
    assert!(claude["mcpServers"].get("shared").is_some());
}

#[test]
fn sync_preserves_native_fields_in_all_json_agent_formats() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{}}"#).unwrap();
    let fixtures = [
        (".claude.json", "mcpServers"),
        (".cursor/mcp.json", "mcpServers"),
        (".copilot/mcp-config.json", "mcpServers"),
        (".gemini/antigravity/mcp_config.json", "mcpServers"),
        (".qwen/settings.json", "mcpServers"),
        ("AppData/Roaming/Code/User/mcp.json", "servers"),
    ];
    for (file, section) in fixtures {
        let path = home.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_string(&serde_json::json!({section: {"local": {"command":"node", "args":["one.js"], "env":{"TOKEN":"value"}, "headers":{"AUTH":"key"}, "enabled":false, "timeout":1000}}})).unwrap()).unwrap();
    }

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();

    for (file, section) in fixtures {
        let data: serde_json::Value = serde_json::from_str(&fs::read_to_string(home.join(file)).unwrap()).unwrap();
        assert_eq!(data[section]["local"]["env"]["TOKEN"], "value", "{file}");
        assert_eq!(data[section]["local"]["headers"]["AUTH"], "key", "{file}");
        assert_eq!(data[section]["local"]["enabled"], false, "{file}");
        assert_eq!(data[section]["local"]["timeout"], 1000, "{file}");
    }
}

#[test]
fn sync_preserves_zcode_env_and_timeout() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    let zcode = home.join(".zcode/cli/config.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::create_dir_all(zcode.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{}}"#).unwrap();
    fs::write(&zcode, r#"{"mcp":{"servers":{"local":{"type":"local","command":["node"],"env":{"TOKEN":"value"},"timeout":1000}}}}"#).unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();

    let result: serde_json::Value = serde_json::from_str(&fs::read_to_string(zcode).unwrap()).unwrap();
    assert_eq!(result["mcp"]["servers"]["local"]["env"]["TOKEN"], "value");
    assert_eq!(result["mcp"]["servers"]["local"]["timeout"], 1000);
}

#[test]
fn sync_does_not_write_when_a_native_server_would_lose_fields() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    let codex = home.join(".codex/config.toml");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{}}"#).unwrap();
    let original = "[mcp_servers.local]\ncommand = \"node\"\nargs = [\"index.js\"]\ncustom = \"keep\"\n";
    fs::write(&codex, original).unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().failure();

    assert_eq!(fs::read_to_string(codex).unwrap(), original);
    assert_eq!(fs::read_to_string(canonical).unwrap(), r#"{"mcp":{}}"#);
}

#[test]
fn sync_rolls_back_canonical_if_an_agent_file_cannot_be_written() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{}}"#).unwrap();
    let opencode = home.join(".config/opencode/opencode.json");
    fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    fs::write(&opencode, r#"{"mcp":{"local":{"type":"local","command":["node"]}}}"#).unwrap();
    let blocked = home.join(".grok");
    fs::write(&blocked, "not a directory").unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().failure();

    assert_eq!(fs::read_to_string(canonical).unwrap(), r#"{"mcp":{}}"#);
    assert_eq!(fs::read_to_string(opencode).unwrap(), r#"{"mcp":{"local":{"type":"local","command":["node"]}}}"#);
}

#[test]
fn sync_keeps_manual_canonical_changes_when_agent_is_unchanged() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    let opencode = home.join(".config/opencode/opencode.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{"shared":{"type":"local","command":["node","old.js"]}}}"#).unwrap();
    fs::write(&opencode, r#"{"mcp":{"shared":{"type":"local","command":["node","old.js"]}}}"#).unwrap();
    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();
    let mut state: serde_json::Value = serde_json::from_str(&fs::read_to_string(&canonical).unwrap()).unwrap();
    state["mcp"]["shared"]["command"] = serde_json::json!(["node", "new.js"]);
    fs::write(&canonical, serde_json::to_string(&state).unwrap()).unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();

    let opencode: serde_json::Value = serde_json::from_str(&fs::read_to_string(opencode).unwrap()).unwrap();
    assert_eq!(opencode["mcp"]["shared"]["command"], serde_json::json!(["node", "new.js"]));
}

#[test]
fn deleting_canonical_shared_server_does_not_resurrect_it_from_published_agents() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{"shared":{"type":"local","command":["node"]}}}"#).unwrap();
    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();
    let mut state: serde_json::Value = serde_json::from_str(&fs::read_to_string(&canonical).unwrap()).unwrap();
    state["mcp"].as_object_mut().unwrap().remove("shared");
    fs::write(&canonical, serde_json::to_string(&state).unwrap()).unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().success();

    let state: serde_json::Value = serde_json::from_str(&fs::read_to_string(&canonical).unwrap()).unwrap();
    let claude: serde_json::Value = serde_json::from_str(&fs::read_to_string(home.join(".claude.json")).unwrap()).unwrap();
    assert!(state["agents"]["claude"].get("shared").is_none());
    assert!(claude["mcpServers"].get("shared").is_none());
}

#[test]
fn sync_rejects_invalid_agent_before_touching_canonical() {
    let sandbox = TempDir::new().unwrap();
    let home = sandbox.path();
    let canonical = home.join(".agents/mcp_config.json");
    let opencode = home.join(".config/opencode/opencode.json");
    fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    fs::write(&canonical, r#"{"mcp":{}}"#).unwrap();
    fs::write(&opencode, r#"{"mcp":{"playwright":{"type":"local","command":["node"]}}}"#).unwrap();
    fs::write(home.join(".claude.json"), "not-json").unwrap();

    let mut cmd = Command::cargo_bin("agentalign").unwrap();
    with_home(&mut cmd, home).arg("sync").assert().failure();

    assert_eq!(fs::read_to_string(canonical).unwrap(), r#"{"mcp":{}}"#);
    assert_eq!(fs::read_to_string(opencode).unwrap(), r#"{"mcp":{"playwright":{"type":"local","command":["node"]}}}"#);
}
