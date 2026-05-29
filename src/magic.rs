//! Magic mode management: LaunchAgent install/uninstall/status.
//!
//! On macOS, creates a LaunchAgent plist at
//! `~/Library/LaunchAgents/com.agentalign.magic.plist` that runs
//! `agentalign watch` on login and keeps it alive.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const PLIST_LABEL: &str = "com.agentalign.magic";

/// Get the path to the LaunchAgent plist.
fn plist_path() -> PathBuf {
    dirs::home_dir()
        .expect("HOME must be set")
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", PLIST_LABEL))
}

/// Get the path to the agentalign binary.
fn binary_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("agentalign"))
}

/// Generate the LaunchAgent plist XML.
fn generate_plist(binary: &std::path::Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>watch</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/agentalign.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/agentalign.err</string>
</dict>
</plist>
"#,
        PLIST_LABEL,
        binary.display()
    )
}

/// Enable magic mode: install and start the LaunchAgent.
pub fn enable() -> anyhow::Result<()> {
    let plist = plist_path();
    let binary = binary_path();

    // Ensure LaunchAgents directory exists
    if let Some(parent) = plist.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write plist
    let xml = generate_plist(&binary);
    fs::write(&plist, xml)?;

    // Load with launchctl
    let output = Command::new("launchctl")
        .args(["load", "-w", &plist.to_string_lossy()])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("launchctl load failed: {}", stderr);
    }

    println!("Magic mode enabled. LaunchAgent installed at {}", plist.display());
    println!("Daemon will start automatically on login.");
    println!("Logs: /tmp/agentalign.log /tmp/agentalign.err");

    Ok(())
}

/// Disable magic mode: unload and remove the LaunchAgent.
pub fn disable() -> anyhow::Result<()> {
    let plist = plist_path();

    if plist.exists() {
        // Unload with launchctl
        let output = Command::new("launchctl")
            .args(["unload", "-w", &plist.to_string_lossy()])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("launchctl unload warning: {}", stderr);
        }

        // Remove plist
        fs::remove_file(&plist)?;
        println!("Magic mode disabled. LaunchAgent removed.");
    } else {
        println!("Magic mode is already disabled.");
    }

    Ok(())
}

/// Show magic mode status.
pub fn status() -> anyhow::Result<()> {
    let plist = plist_path();

    if !plist.exists() {
        println!("Magic mode: OFF");
        println!("Run `agentalign magic on` to enable automatic bidirectional sync.");
        return Ok(());
    }

    // Check if the service is loaded
    let output = Command::new("launchctl")
        .args(["list", PLIST_LABEL])
        .output()?;

    let loaded = output.status.success();

    println!("Magic mode: {}", if loaded { "ON (running)" } else { "ON (not running)" });
    println!("LaunchAgent: {}", plist.display());
    println!("Binary: {}", binary_path().display());
    println!("Logs: /tmp/agentalign.log /tmp/agentalign.err");

    if loaded {
        // Try to get PID
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains("PID") {
                println!("{}", line.trim());
                break;
            }
        }
    }

    Ok(())
}
