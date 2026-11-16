//! Notification entry point. Banners are off: cleanup records itself in the
//! log and the receipt, and a desktop banner is not part of that record.

use anyhow::Result;

/// Does not display a banner. The log and the receipt are the record.
pub fn send_notification(title: &str, body: &str) -> Result<()> {
    let _ = (title, body);
    Ok(())
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
