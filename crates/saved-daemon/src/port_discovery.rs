//! Port discovery mechanism for daemon control

use anyhow::Result;
use std::path::PathBuf;
use std::fs;

const PORT_FILE_NAME: &str = "control.port";

pub fn save_control_port(account_path: &PathBuf, port: u16) -> Result<()> {
    let port_file = account_path.join(PORT_FILE_NAME);
    fs::write(port_file, port.to_string())?;
    Ok(())
}

pub fn load_control_port(account_path: &PathBuf) -> Result<Option<u16>> {
    let port_file = account_path.join(PORT_FILE_NAME);
    
    if !port_file.exists() {
        return Ok(None);
    }
    
    let content = fs::read_to_string(port_file)?;
    let port: u16 = content.trim().parse()?;
    Ok(Some(port))
}

pub fn cleanup_control_port(account_path: &PathBuf) -> Result<()> {
    let port_file = account_path.join(PORT_FILE_NAME);
    if port_file.exists() {
        fs::remove_file(port_file)?;
    }
    Ok(())
}
