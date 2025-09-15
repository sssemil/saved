//! Text formatting utilities

use colored::*;

/// Format a file size in a human-readable way
pub fn format_file_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    const THRESHOLD: u64 = 1024;

    if bytes < THRESHOLD {
        format!("{} {}", bytes, UNITS[0])
    } else {
        let mut size = bytes as f64;
        let mut unit_index = 0;

        while size >= THRESHOLD as f64 && unit_index < UNITS.len() - 1 {
            size /= THRESHOLD as f64;
            unit_index += 1;
        }

        if size.fract() == 0.0 {
            format!("{:.0} {}", size, UNITS[unit_index])
        } else {
            format!("{:.1} {}", size, UNITS[unit_index])
        }
    }
}

/// Format a message ID for display
pub fn format_message_id(msg_id: &[u8; 32]) -> String {
    hex::encode(msg_id)
}

/// Format a short message ID for display
pub fn format_short_message_id(msg_id: &[u8; 32]) -> String {
    let full_hex = hex::encode(msg_id);
    format!("{}...{}", &full_hex[..8], &full_hex[56..])
}

/// Print a success message
pub fn print_success(message: &str) {
    println!("{}", format!("✓ {}", message).green().bold());
}

/// Print a warning message
pub fn print_warning(message: &str) {
    println!("{}", format!("⚠️  {}", message).yellow().bold());
}

/// Print a section header
pub fn print_section_header(title: &str) {
    println!();
    println!("{}", title.bright_blue().bold());
    println!("{}", "=".repeat(title.len()).bright_blue());
}
