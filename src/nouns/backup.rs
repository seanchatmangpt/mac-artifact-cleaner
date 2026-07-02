//! iOS device backup noun.

use clap::Subcommand;

use crate::integration::progress::human_bytes as format_bytes;

#[derive(Subcommand, Debug)]
pub enum BackupAction {
    /// List iOS device backups with sizes and dates
    List,
    /// Show total backup storage used
    Summary,
}

pub fn handle(action: BackupAction) -> anyhow::Result<()> {
    match action {
        BackupAction::List => handle_list(),
        BackupAction::Summary => handle_summary(),
    }
}

fn handle_list() -> anyhow::Result<()> {
    use crate::integration::backup::scan_ios_backups;

    let backups = scan_ios_backups()?;

    if backups.is_empty() {
        println!("No iOS backups found at ~/Library/Application Support/MobileSync/Backup");
        return Ok(());
    }

    println!("iOS Backups ({} found):", backups.len());
    println!("{:-<60}", "");
    for b in &backups {
        let device = b.device_name.as_deref().unwrap_or("Unknown Device");
        let product = b.product_type.as_deref().unwrap_or("Unknown Model");
        let date = b.last_backup_date.as_deref().unwrap_or("unknown date");

        println!("Device: {} ({})", device, product);
        println!("  Backup ID:   {}", b.id);
        println!("  Last backup: {}", date);
        println!("  Size:        {}", format_bytes(b.size_bytes));
        println!();
    }

    Ok(())
}

fn handle_summary() -> anyhow::Result<()> {
    use crate::integration::backup::scan_ios_backups;

    let backups = scan_ios_backups()?;

    if backups.is_empty() {
        println!("No iOS backups found at ~/Library/Application Support/MobileSync/Backup");
        return Ok(());
    }

    let total: u64 = backups.iter().map(|b| b.size_bytes).sum();
    println!("iOS Backup Summary");
    println!("{:-<40}", "");
    println!("Count: {} backup(s)", backups.len());
    println!("Total: {}", format_bytes(total));

    Ok(())
}
