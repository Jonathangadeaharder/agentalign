//! Magic mode management: LaunchAgent install/uninstall/status.
//!
//! On macOS, creates a LaunchAgent plist at
//! `~/Library/LaunchAgents/com.agentalign.magic.plist` that runs
//! `agentalign watch` on login and keeps it alive.

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
use anyhow::Context;

#[cfg(unix)]
const PLIST_LABEL: &str = "com.agentalign.magic";

/// Get the path to the LaunchAgent plist.
#[cfg(unix)]
fn plist_path() -> anyhow::Result<PathBuf> {
    let home = crate::shared::home_dir()?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", PLIST_LABEL)))
}

/// Get the path to the agentalign binary.
#[cfg(unix)]
fn binary_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("agentalign"))
}

/// Get the current user's GUI domain UID for launchctl bootstrap.
#[cfg(unix)]
fn gui_uid() -> String {
    // On macOS, the GUI domain is gui/<uid> where uid is the user's numeric ID
    // SAFETY: `getuid()` is a POSIX function that always returns the real user ID
    // of the calling process. It performs no memory operations, has no preconditions,
    // and cannot fail. The call is thread-safe and has no side effects.
    let uid = unsafe { libc::getuid() };
    format!("gui/{}", uid)
}

/// Magic mode is a macOS LaunchAgent; there is no equivalent on other platforms yet.
#[cfg(not(unix))]
fn unsupported() -> anyhow::Error {
    anyhow::anyhow!(
        "magic mode is macOS-only (it installs a LaunchAgent); run `agentalign watch` manually instead"
    )
}

/// Escape a string for safe XML/plist embedding.
#[cfg(unix)]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Generate the LaunchAgent plist XML.
#[cfg(unix)]
fn generate_plist(binary: &std::path::Path) -> String {
    let escaped_binary = xml_escape(&binary.display().to_string());
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
        escaped_binary
    )
}

/// Enable magic mode: install and start the LaunchAgent.
#[cfg(unix)]
pub fn enable() -> anyhow::Result<()> {
    let plist = plist_path()?;
    let binary = binary_path();

    // Ensure LaunchAgents directory exists
    if let Some(parent) = plist.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write plist
    let xml = generate_plist(&binary);
    fs::write(&plist, xml)?;

    // Try modern launchctl bootstrap first (macOS 13+), fall back to load
    let domain = gui_uid();
    let output = Command::new("launchctl")
        .args(["bootstrap", &domain, &plist.to_string_lossy()])
        .output();

    match output {
        Ok(o) if o.status.success() => {}
        _ => {
            // Fallback to legacy load
            let output = Command::new("launchctl")
                .args(["load", "-w", &plist.to_string_lossy()])
                .output()
                .context("Failed to run launchctl")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("launchctl load failed: {}", stderr);
            }
        }
    }

    println!(
        "Magic mode enabled. LaunchAgent installed at {}",
        plist.display()
    );
    println!("Daemon will start automatically on login.");
    println!("Logs: /tmp/agentalign.log /tmp/agentalign.err");

    Ok(())
}

/// Disable magic mode: unload and remove the LaunchAgent.
#[cfg(unix)]
pub fn disable() -> anyhow::Result<()> {
    let plist = plist_path()?;

    if plist.exists() {
        // Try modern bootout first, fall back to unload
        let domain_target = format!("{}/{}", gui_uid(), PLIST_LABEL);
        let output = Command::new("launchctl")
            .args(["bootout", &domain_target])
            .output();

        match output {
            Ok(o) if o.status.success() => {}
            _ => {
                // Fallback to legacy unload
                let output = Command::new("launchctl")
                    .args(["unload", "-w", &plist.to_string_lossy()])
                    .output()
                    .context("Failed to run launchctl")?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!("launchctl unload warning: {}", stderr);
                }
            }
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
#[cfg(unix)]
pub fn status() -> anyhow::Result<()> {
    let plist = plist_path()?;

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

    println!(
        "Magic mode: {}",
        if loaded {
            "ON (running)"
        } else {
            "ON (not running)"
        }
    );
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

/// Magic mode stubs for platforms without launchd.
#[cfg(not(unix))]
pub fn enable() -> anyhow::Result<()> {
    Err(unsupported())
}

#[cfg(not(unix))]
pub fn disable() -> anyhow::Result<()> {
    Err(unsupported())
}

#[cfg(not(unix))]
pub fn status() -> anyhow::Result<()> {
    println!("Magic mode: unsupported on this platform");
    Ok(())
}
