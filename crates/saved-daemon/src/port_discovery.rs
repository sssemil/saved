//! Port discovery mechanism for daemon control

use anyhow::Result;
use std::fs;
use std::path::PathBuf;

const PORT_FILE_NAME: &str = "control.port";

pub fn save_control_port(account_path: &PathBuf, port: u16) -> Result<()> {
    let port_file = account_path.join(PORT_FILE_NAME);
    fs::write(port_file, port.to_string())?;
    Ok(())
}

pub fn cleanup_control_port(account_path: &PathBuf) -> Result<()> {
    let port_file = account_path.join(PORT_FILE_NAME);
    if port_file.exists() {
        fs::remove_file(port_file)?;
    }
    Ok(())
}
