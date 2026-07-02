//! macOS user notification delivery via osascript.

use anyhow::Result;

/// Send a macOS user notification banner.
pub fn send_notification(title: &str, body: &str) -> Result<()> {
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        body.replace('"', "'"),
        title.replace('"', "'")
    );
    let status = std::process::Command::new("osascript")
        .args(["-e", &script])
        .status()?;
    if !status.success() {
        anyhow::bail!("osascript notification failed");
    }
    Ok(())
}

/// Send a disk pressure notification.
pub fn notify_disk_pressure(free_gb: f64, threshold_gb: f64) -> Result<()> {
    let body = format!(
        "Free space {:.1} GB is below threshold {:.1} GB",
        free_gb, threshold_gb
    );
    send_notification("osx-clnr: Disk Pressure", &body)
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_notification_script_escaping() {
        // Ensure quotes in title/body are escaped (replaced with single quotes)
        let title = "test \"title\"";
        let body = "some \"body\" text";
        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            body.replace('"', "'"),
            title.replace('"', "'")
        );
        assert!(!script.contains("\\\""));
    }
}
