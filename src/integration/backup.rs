//! iOS device backup scanning integration.

use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IosBackup {
    pub id: String,
    pub device_name: Option<String>,
    pub product_type: Option<String>,
    pub last_backup_date: Option<String>,
    pub size_bytes: u64,
    pub path: PathBuf,
}

/// Scan `~/Library/Application Support/MobileSync/Backup` for iOS device backups.
pub fn scan_ios_backups() -> Result<Vec<IosBackup>> {
    let base = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library/Application Support/MobileSync/Backup");

    if !base.exists() {
        return Ok(Vec::new());
    }

    let mut backups = Vec::new();

    for entry in std::fs::read_dir(&base)?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }

        let info_plist = path.join("Info.plist");
        let (device_name, product_type, last_backup_date) =
            if info_plist.exists() { parse_info_plist(&info_plist) } else { (None, None, None) };

        let size_bytes = du_path(&path);

        backups.push(IosBackup {
            id,
            device_name,
            product_type,
            last_backup_date,
            size_bytes,
            path,
        });
    }

    // Sort by size descending for convenience.
    backups.sort_by_key(|b| std::cmp::Reverse(b.size_bytes));

    Ok(backups)
}

/// Extract device metadata from an `Info.plist` file using line-by-line string parsing.
fn parse_info_plist(path: &std::path::Path) -> (Option<String>, Option<String>, Option<String>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return (None, None, None),
    };

    let device_name = extract_plist_string(&content, "Device Name");
    let product_type = extract_plist_string(&content, "Product Type");
    let last_backup_date = extract_plist_date(&content, "Last Backup Date");

    (device_name, product_type, last_backup_date)
}

/// Find `<key>KEY</key>` then return the text inside `<string>...</string>` on the next line.
fn extract_plist_string(content: &str, key: &str) -> Option<String> {
    let key_tag = format!("<key>{}</key>", key);
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        if line.contains(&key_tag) {
            if let Some(next) = lines.next() {
                let trimmed = next.trim();
                if trimmed.starts_with("<string>") && trimmed.ends_with("</string>") {
                    let value = &trimmed["<string>".len()..trimmed.len() - "</string>".len()];
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Find `<key>KEY</key>` then return the text inside `<date>...</date>` on the next line.
fn extract_plist_date(content: &str, key: &str) -> Option<String> {
    let key_tag = format!("<key>{}</key>", key);
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        if line.contains(&key_tag) {
            if let Some(next) = lines.next() {
                let trimmed = next.trim();
                if trimmed.starts_with("<date>") && trimmed.ends_with("</date>") {
                    let value = &trimmed["<date>".len()..trimmed.len() - "</date>".len()];
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Estimate directory size via `du -sk`.
fn du_path(path: &std::path::Path) -> u64 {
    let output = std::process::Command::new("du").args(["-sk", &path.to_string_lossy()]).output();
    if let Ok(out) = output {
        if let Ok(s) = String::from_utf8(out.stdout) {
            if let Some(kb) = s.split_whitespace().next() {
                if let Ok(n) = kb.parse::<u64>() {
                    return n * 1024;
                }
            }
        }
    }
    0
}
