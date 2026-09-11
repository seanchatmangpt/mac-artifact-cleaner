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

/// Find `<key>KEY</key>` then return the text inside `<TAG>...</TAG>` on the next line.
fn extract_plist_tagged(content: &str, key: &str, tag: &str) -> Option<String> {
    let key_tag = format!("<key>{}</key>", key);
    let open_tag = format!("<{}>", tag);
    let close_tag = format!("</{}>", tag);
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        if line.contains(&key_tag) {
            if let Some(next) = lines.next() {
                let trimmed = next.trim();
                if trimmed.starts_with(&open_tag) && trimmed.ends_with(&close_tag) {
                    let value = &trimmed[open_tag.len()..trimmed.len() - close_tag.len()];
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Find `<key>KEY</key>` then return the text inside `<string>...</string>` on the next line.
fn extract_plist_string(content: &str, key: &str) -> Option<String> {
    extract_plist_tagged(content, key, "string")
}

/// Find `<key>KEY</key>` then return the text inside `<date>...</date>` on the next line.
fn extract_plist_date(content: &str, key: &str) -> Option<String> {
    extract_plist_tagged(content, key, "date")
}

/// Estimate directory size via `du -sk`. Silently returns 0 on any failure.
///
/// Thin alias over the single shared implementation in
/// [`crate::integration::progress::du_bytes`].
fn du_path(path: &std::path::Path) -> u64 {
    crate::integration::progress::du_bytes(path).unwrap_or(0)
}
